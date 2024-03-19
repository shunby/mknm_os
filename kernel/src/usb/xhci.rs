use core::{
    alloc::{AllocError, Allocator, Layout},
    arch::asm,
    fmt::{self, Formatter},
    mem::{size_of, transmute, ManuallyDrop, MaybeUninit},
    ptr::{read_volatile, write_volatile, NonNull},
    slice::from_raw_parts_mut,
};

use alloc::{
    alloc::{alloc, Global},
    boxed::Box,
    collections::BTreeMap,
    sync::Arc,
    vec::Vec,
};
use bitfield::bitfield;
use futures::{
    channel::oneshot::Receiver as OneshotReceiver,
    channel::oneshot::{self, channel, Sender as OneshotSender},
};
use num_traits::cast::FromPrimitive;
use xhci::{
    accessor::Mapper,
    context,
    context::{
        Device32Byte, Device64Byte, DeviceHandler, EndpointHandler, EndpointState, Input,
        Input32Byte, Input64Byte, InputHandler, SlotHandler,
    },
    registers::{InterrupterRegisterSet, PortRegisterSet},
    ring::trb::{
        self,
        transfer::{Direction, TransferType},
    },
    ring::trb::{
        command::{AddressDevice, EnableSlot},
        event::{CommandCompletion, CompletionCode, PortStatusChange, TransferEvent},
        transfer::{self, DataStage, SetupStage, StatusStage},
        Type,
    },
    Registers,
};

use crate::{
    memory_manager::{LazyInit, Mutex},
    pci::PCIDevice,
    usb::{
        runtime::{new_channel, new_executor_and_spawner},
        usbd::UsbDriver,
    },
    println,
};

use super::{
    class::mouse::MouseReport,
    ring::{EventRing, ProducerRing},
    runtime::{new_broadcast_channel, BroadcastReceiver, Executor, Sender, Spawner},
};

static XHCI_REGS: LazyInit<Registers<LinearMapper>> = LazyInit::new();
static INT_REGS: LazyInit<InterrupterRegisterSet<LinearMapper>> = LazyInit::new();
static EXECUTOR: LazyInit<Executor<'static, Result<(), XhciError>>> = LazyInit::new();
pub static SPAWNER: LazyInit<Spawner<'static, Result<(), XhciError>>> = LazyInit::new();

pub fn with_regs<R>(f: impl FnOnce(&mut Registers<LinearMapper>) -> R) -> R {
    let mut lock = XHCI_REGS.lock();
    f(&mut lock)
}

pub fn read_port(port_id: usize) -> PortRegisterSet {
    with_regs(|r| r.port_register_set.read_volatile_at(port_id))
}

pub fn update_port<U: FnOnce(&mut PortRegisterSet)>(port_id: usize, f: U) {
    with_regs(|r| r.port_register_set.update_volatile_at(port_id, f));
}

pub fn ring_doorbell(i: usize, target: u8) {
    with_regs(|r| {
        r.doorbell.update_volatile_at(i, |bell| {
            bell.set_doorbell_target(target);
        })
    });
}

pub fn run_xhci_tasks() {
    XHCI.lock().process_events();
    let mut executor = EXECUTOR.lock();
    while executor.has_next_task() {
        if let Some(Err(e)) = executor.process_next_task().unwrap() {
            println!("Error while running xHCI tasks: {e:?}");
        }
    }
}

pub async fn emit_command_async(trb: UnknownTRB) -> Result<CommandCompletion, XhciError> {
    let e = XHCI.lock().clone();
    e.emit_command_async(trb).await
}

pub fn push_transfer_trb_async(
    slot_id: usize,
    endpoint_id: usize,
    trb: transfer::Allowed,
) -> Result<Option<OneshotReceiver<Result<TransferEvent, TransferEvent>>>, XhciError> {
    XHCI
        .lock()
        .push_transfer_trb_async(slot_id, endpoint_id, trb)
}

pub struct AlignedAlloc<const N: usize> {}

unsafe impl<const N: usize> Allocator for AlignedAlloc<N> {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        Global.allocate(layout.align_to(N).unwrap())
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        Global.deallocate(ptr, layout.align_to(N).unwrap())
    }
}

#[derive(Debug)]
pub enum XhciError {
    InvalidTrb,
    RingIsFull,
    InvalidCommandCompletionTrb,
    AddressDeviceCommandFailed(CommandCompletion),
    UnexpectedDescriptor,
    TransferError(TransferEvent),
}

pub(super) static XHCI: LazyInit<Arc<XhciController>> = LazyInit::new();

#[repr(C)]
struct XhciCapability {
    cap_id: u8,
    next_cap_ptr: u8,
    cap_specific: u16,
}

impl XhciCapability {
    fn next(&self) -> Option<*mut XhciCapability> {
        if self.next_cap_ptr == 0 {
            None
        } else {
            let ptr =
                (self as *const Self as u64 + self.next_cap_ptr as u64 * 4) as *mut XhciCapability;
            Some(ptr)
        }
    }
}

fn ownership_handoff(regs: &Registers<LinearMapper>, mmio_base: u64) {
    let ex_cap_ptr = regs
        .capability
        .hccparams1
        .read_volatile()
        .xhci_extended_capabilities_pointer() as u64;
    let mut cap = unsafe { &mut *((mmio_base + ex_cap_ptr * 4) as *mut XhciCapability) };

    let usb_leg_sup = loop {
        if cap.cap_id == 1 {
            break cap;
        }
        match cap.next() {
            Some(next) => {
                cap = unsafe { &mut *next };
            }
            None => {
                return;
            }
        }
    };

    {
        let cap_specific = &mut usb_leg_sup.cap_specific;
        let os_owned_semaphore = (*cap_specific >> 8) & 0b1;
        if os_owned_semaphore == 1 {
            return;
        }
        unsafe {
            write_volatile(cap_specific as *mut u16, *cap_specific | 0b100000000);
        }

        loop {
            let spec = unsafe { read_volatile(cap_specific as *const u16) };
            let bios_owned_semaphore = spec & 0b1;
            let os_owned_semaphore = (spec >> 8) & 0b1;
            if bios_owned_semaphore == 0 && os_owned_semaphore == 1 {
                break;
            }
        }
    }
    let ctl_sts_ptr = (usb_leg_sup as *const XhciCapability as u64 + 4) as *const u32;
    let mut ctl_sts = unsafe { read_volatile(ctl_sts_ptr) };

    // turn off all SMIs
    ctl_sts &= 0xffff1fee;
    ctl_sts |= 0xe0000000;
    unsafe {
        write_volatile(ctl_sts_ptr as *mut u32, ctl_sts);
    }
}

fn find_lsb(bits: u16) -> usize {
    for i in 0..15 {
        if (bits >> i) & 1 == 1 {
            return i;
        }
    }
    16
}

pub unsafe fn initialize_xhci(
    xhc: PCIDevice,
    intel_ehci_found: bool,
    mouse_callback: impl Fn(Box<MouseReport>) + Send + 'static,
) {
    let xhc_bar = xhc.read_bar(0);
    let mmio_base = (xhc_bar & !0b1111_u64) as usize;

    let mut regs = xhci::Registers::new(mmio_base, LinearMapper {});

    ownership_handoff(&regs, mmio_base as u64);

    if intel_ehci_found {
        println!("Switching eHCI ports to xHCI");
        let ports_available = xhc.read_confreg(0xD4); // read XUSB2PRM
        xhc.write_confreg(0xD0, ports_available); // write XUSB2PR
    }

    println!("Initializing xHCI...");

    let op = &mut regs.operational;

    op.usbcmd.update_volatile(|x| {
        x.clear_interrupter_enable();
        x.clear_host_system_error_enable();
        x.clear_enable_wrap_event();
    });
    if !op.usbsts.read_volatile().hc_halted() {
        println!("Stopping HC...");
        op.usbcmd.update_volatile(|r| {
            r.clear_run_stop();
        });
        while !op.usbsts.read_volatile().hc_halted() {}
    }

    println!("Resetting HC...");
    op.usbcmd.update_volatile(|x| {
        x.set_host_controller_reset();
    });
    // Intel® 8/C220 Series Chipset may hung if registers are accessed within 1ms from hc reset
    // wait_for(20);
    while op.usbcmd.read_volatile().host_controller_reset() {}
    while op.usbsts.read_volatile().controller_not_ready() {}

    let max_slots = regs
        .capability
        .hcsparams1
        .read_volatile()
        .number_of_device_slots();
    let num_ports = regs.capability.hcsparams1.read_volatile().number_of_ports();
    let ctx_size = if regs.capability.hccparams1.read_volatile().context_size() {
        ContextSize::Csz64Bytes
    } else {
        ContextSize::Csz32Bytes
    };
    println!("max_slots={max_slots}");
    println!("num_ports={num_ports}");
    println!("ctx_size={ctx_size:?}");

    op.config.update_volatile(|cfg| {
        cfg.set_max_device_slots_enabled(max_slots);
    });
    println!("1");

    let num_scratch_pads = regs
        .capability
        .hcsparams2
        .read_volatile()
        .max_scratchpad_buffers() as usize;
    let pagesize_bit = find_lsb(op.pagesize.read_volatile().get());
    let page_size = 1 << (12 + pagesize_bit);
    println!("scratch_pads={num_scratch_pads}");
    println!("page_size={page_size}B");

    let mut dcbaa: Box<[u64]> = unsafe {
        let mut bytes = aligned_bytes::<u64>((max_slots as usize + 1) * 8, 64);
        bytes.copy_from_slice(&vec![MaybeUninit::new(0); bytes.len()]);
        transmute(bytes)
    };

    let scratchpad_buf_arr = if num_scratch_pads > 0 {
        let mut page_ptrs: Vec<u64, AlignedAlloc<64>> =
            Vec::with_capacity_in(num_scratch_pads, AlignedAlloc::<64> {});
        for _ in 0..num_scratch_pads {
            unsafe {
                let page = alloc(Layout::from_size_align(page_size, page_size).unwrap());
                if page.is_null() {
                    panic!("Failed to allocate xHCI scratchpad buffer");
                }
                from_raw_parts_mut(page, page_size).fill(0);
                page_ptrs.push(page as u64);
            }
        }
        let scratchpad_buf_arr = page_ptrs.into_boxed_slice();

        dcbaa[0] = scratchpad_buf_arr.as_ref() as *const [u64] as *const u64 as u64;

        Some(scratchpad_buf_arr)
    } else {
        None
    };

    println!("2");

    op.dcbaap
        .update_volatile(|x| x.set(dcbaa.as_mut_ptr() as u64));
    println!("3");

    let cmd_ring = ProducerRing::new(32);
    op.crcr.update_volatile(|x| {
        x.set_command_ring_pointer(cmd_ring.get_buf_ptr());
        x.set_ring_cycle_state();
    });
    println!("4");

    let event_ring = EventRing::new(32);
    let mut entry = EventRingSegmentTableEntry([0; 2]);
    entry.set_base_addr(event_ring.get_buf_ptr());
    entry.set_ring_segment_size(event_ring.size() as u64);
    println!("5");

    let er_table: ManuallyDrop<Vec<EventRingSegmentTableEntry<[u64; 2]>>> =
        ManuallyDrop::new(vec![entry]);
    let mut ints = InterrupterRegisterSet::new(
        mmio_base,
        regs.capability.rtsoff.read_volatile(),
        LinearMapper {},
    );
    println!("6");

    let mut iregs = ints.interrupter_mut(0);
    iregs
        .erstsz
        .update_volatile(|x| x.set(er_table.len() as u16));
    iregs
        .erstba
        .update_volatile(|x| x.set(er_table.as_ptr() as u64));
    iregs.erdp.update_volatile(|x| {
        x.set_0_event_handler_busy();
        x.set_event_ring_dequeue_pointer(event_ring.get_buf_ptr())
    });

    iregs.imod.update_volatile(|x| {
        x.set_interrupt_moderation_interval(4000);
    });
    iregs.iman.update_volatile(|x| {
        x.clear_interrupt_pending();
        x.set_interrupt_enable();
    });
    println!("7");

    op.usbcmd.update_volatile(|x| {
        x.set_interrupter_enable();
    });
    println!("8");
    op.usbcmd.update_volatile(|x| {
        x.set_run_stop();
    });
    while op.usbsts.read_volatile().hc_halted() {}
    println!("8");

    INT_REGS.lock().init(ints);
    XHCI_REGS.lock().init(regs);
    let (executor, spawner) = new_executor_and_spawner::<Result<(), XhciError>>();
    EXECUTOR.lock().init(executor);
    SPAWNER.lock().init(spawner);
    let (new_conn_sender, new_conn_receiver) = new_channel::<usize>();
    let (addr_sender, addr_receiver) = new_channel::<usize>();

    XHCI.lock().init(Arc::new(XhciController {
        event_ring: Mutex::new(event_ring),
        cmd_ring: Mutex::new(cmd_ring),
        trf_rings: Mutex::new(BTreeMap::new()),
        cmd_callbacks: Mutex::new(BTreeMap::new()),
        trb_callbacks: Mutex::new(BTreeMap::new()),
        port_reset_callbacks: Mutex::new(BTreeMap::new()),
        new_connection_callback: Mutex::new(new_conn_sender),
        address_device_callback: Mutex::new(addr_sender),
        dcbaa: Mutex::new(dcbaa),
        scratchpad_buf_arr,
        devices: Mutex::new(BTreeMap::default()),
        ctx_size,
        port_status: Mutex::new(vec![PortStatus::Disconnected; num_ports as usize]),
        addressing_port: Mutex::new(None),
    }));

    let mut usbd = UsbDriver::new(addr_receiver, Box::new(mouse_callback));

    SPAWNER.lock().spawn(async move { usbd.main_loop().await });

    SPAWNER.lock().spawn(async move {
        let e = XHCI.lock().clone();

        for port_id in 0..num_ports {
            if read_port(port_id as usize).portsc.current_connect_status() {
                update_port(port_id as usize, |p| {
                    p.portsc.clear_connect_status_change();
                });
                e.init_device_exclusive_async(port_id as usize).await?;
            }
        }

        loop {
            let port_id = new_conn_receiver.receive_async().await;
            if e.port_status.lock()[port_id as usize] == PortStatus::Disconnected {
                e.init_device_exclusive_async(port_id as usize).await?;
            }
        }
    });
    println!("xHCI initialization complete");
    run_xhci_tasks();
}
unsafe fn aligned_bytes<T>(len: usize, align: usize) -> Box<[MaybeUninit<T>]> {
    let n_bytes = len * size_of::<T>();
    let data = from_raw_parts_mut(
        alloc(Layout::from_size_align(n_bytes, align).unwrap()) as *mut MaybeUninit<T>,
        len,
    );
    Box::from_raw(data)
}

bitfield! {
    #[repr(C)]
    struct EventRingSegmentTableEntry ([u64]);
    u64;
    base_addr, set_base_addr: 63,0;
    ring_segment_size, set_ring_segment_size: 79,64;
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum PortStatus {
    Disconnected,
    Connected,
}

#[derive(Debug, Clone, Copy)]
pub enum ContextSize {
    Csz32Bytes,
    Csz64Bytes,
}

impl From<ContextSize> for usize {
    fn from(value: ContextSize) -> Self {
        match value {
            ContextSize::Csz32Bytes => 32,
            ContextSize::Csz64Bytes => 64,
        }
    }
}

pub enum DeviceContext {
    DC32Byte(Box<Device32Byte, AlignedAlloc<64>>),
    DC64Byte(Box<Device64Byte, AlignedAlloc<64>>),
}

impl DeviceContext {
    pub fn new(size: ContextSize) -> Self {
        match size {
            ContextSize::Csz32Bytes => {
                Self::DC32Byte(Box::new_in(context::Device::new_32byte(), AlignedAlloc {}))
            }
            ContextSize::Csz64Bytes => {
                Self::DC64Byte(Box::new_in(context::Device::new_64byte(), AlignedAlloc {}))
            }
        }
    }
    pub fn handler(&self) -> &dyn DeviceHandler {
        match self {
            DeviceContext::DC32Byte(dev) => dev.as_ref(),
            DeviceContext::DC64Byte(dev) => dev.as_ref(),
        }
    }

    pub fn handler_mut(&mut self) -> &mut dyn DeviceHandler {
        match self {
            DeviceContext::DC32Byte(dev) => dev.as_mut(),
            DeviceContext::DC64Byte(dev) => dev.as_mut(),
        }
    }

    pub fn get_address(&self) -> u64 {
        match self {
            DeviceContext::DC32Byte(dev) => dev.as_ref() as *const Device32Byte as u64,
            DeviceContext::DC64Byte(dev) => dev.as_ref() as *const Device64Byte as u64,
        }
    }

    pub fn get_size(&self) -> ContextSize {
        match *self {
            DeviceContext::DC32Byte(_) => ContextSize::Csz32Bytes,
            DeviceContext::DC64Byte(_) => ContextSize::Csz64Bytes,
        }
    }
}

fn dump_ep_ctx(f: &mut Formatter<'_>, ep: &dyn EndpointHandler) -> fmt::Result {
    f.write_fmt(format_args!(
        "state {:?} type {:?} cerr {} interval {} psz {} deq 0x{:x} dcs {}\n",
        ep.endpoint_state(),
        ep.endpoint_type(),
        ep.error_count(),
        ep.interval(),
        ep.max_packet_size(),
        ep.tr_dequeue_pointer(),
        ep.dequeue_cycle_state()
    ))
}
fn dump_slot_ctx(f: &mut Formatter<'_>, slot: &dyn SlotHandler) -> fmt::Result {
    f.write_fmt(format_args!(
        "[slot] speed: {} entries: {} port: {} state: {:?}\n",
        slot.speed(),
        slot.context_entries(),
        slot.root_hub_port_number(),
        slot.slot_state()
    ))
}

impl fmt::Debug for DeviceContext {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        dump_slot_ctx(f, self.handler().slot())?;
        for i in 1..=31 {
            let ep = self.handler().endpoint(i);
            if ep.endpoint_state() != EndpointState::Disabled {
                f.write_fmt(format_args!("[{i}] "))?;
                dump_ep_ctx(f, ep)?;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for InputContext {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let ctrl = self.handler().control();
        f.write_fmt(format_args!("[ctrl] "))?;
        for i in 0..31 {
            if ctrl.add_context_flag(i) {
                f.write_fmt(format_args!("A{i} "))?;
            }
        }
        f.write_fmt(format_args!("\n"))?;

        dump_slot_ctx(f, self.handler().device().slot())?;
        for i in 1..=31 {
            let ep = self.handler().device().endpoint(i);
            if ctrl.add_context_flag(i) {
                f.write_fmt(format_args!("[{i}] "))?;
                dump_ep_ctx(f, ep)?;
            }
        }
        Ok(())
    }
}

pub enum InputContext {
    IC32Byte(Box<Input32Byte, AlignedAlloc<64>>),
    IC64Byte(Box<Input64Byte, AlignedAlloc<64>>),
}

impl InputContext {
    pub fn new(size: ContextSize) -> Self {
        match size {
            ContextSize::Csz32Bytes => {
                Self::IC32Byte(Box::new_in(Input::new_32byte(), AlignedAlloc {}))
            }
            ContextSize::Csz64Bytes => {
                Self::IC64Byte(Box::new_in(Input::new_64byte(), AlignedAlloc {}))
            }
        }
    }

    pub fn handler(&self) -> &dyn InputHandler {
        match self {
            InputContext::IC32Byte(dev) => dev.as_ref(),
            InputContext::IC64Byte(dev) => dev.as_ref(),
        }
    }

    pub fn handler_mut(&mut self) -> &mut dyn InputHandler {
        match self {
            InputContext::IC32Byte(dev) => dev.as_mut(),
            InputContext::IC64Byte(dev) => dev.as_mut(),
        }
    }

    pub fn get_address(&self) -> u64 {
        match self {
            InputContext::IC32Byte(dev) => dev.as_ref() as *const Input32Byte as u64,
            InputContext::IC64Byte(dev) => dev.as_ref() as *const Input64Byte as u64,
        }
    }

    pub fn get_size(&self) -> ContextSize {
        match *self {
            InputContext::IC32Byte(_) => ContextSize::Csz32Bytes,
            InputContext::IC64Byte(_) => ContextSize::Csz64Bytes,
        }
    }
}

pub struct XhciController {
    event_ring: Mutex<EventRing>,
    cmd_ring: Mutex<ProducerRing>,
    trf_rings: Mutex<BTreeMap<usize, BTreeMap<usize, ProducerRing>>>, // index: [slot_id, endpoint_id]

    dcbaa: Mutex<Box<[u64]>>,
    devices: Mutex<BTreeMap<usize, Device>>,
    ctx_size: ContextSize,
    scratchpad_buf_arr: Option<Box<[u64], AlignedAlloc<64>>>,

    cmd_callbacks: Mutex<BTreeMap<u64, OneshotSender<CommandCompletion>>>,
    trb_callbacks: Mutex<BTreeMap<u64, OneshotSender<Result<TransferEvent, TransferEvent>>>>,
    port_reset_callbacks: Mutex<BTreeMap<usize, OneshotSender<PortStatusChange>>>,
    new_connection_callback: Mutex<Sender<usize>>,
    address_device_callback: Mutex<Sender<usize>>,

    port_status: Mutex<Vec<PortStatus>>,
    addressing_port: Mutex<Option<BroadcastReceiver>>,
}

pub enum ControlRequestType {
    GetDescriptor,
    SetConfigutation,
    SetProtocol,
    SetInterface,
}

enum TransferDirection {
    HostToDevice,
    DeviceToHost,
}

impl ControlRequestType {
    fn get_actual_value(&self) -> (u8, u8) {
        match self {
            Self::GetDescriptor => (0b10000000, 6),
            Self::SetConfigutation => (0b00000000, 9),
            Self::SetProtocol => (0b00100001, 11),
            Self::SetInterface => (0b00000001, 11),
        }
    }
}

pub struct SetupData {
    pub request_type: ControlRequestType,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

pub struct Device {
    port_id: usize,
    slot_id: usize,
    out_ctx: DeviceContext,
}

impl Device {
    pub fn context(&self) -> &DeviceContext {
        &self.out_ctx
    }
}

impl XhciController {
    fn process_events(&self) {
        let mut erdp = INT_REGS.lock().interrupter_mut(0).erdp.read_volatile();

        let mut event_ring = self.event_ring.lock();
        while let Some(trb) = event_ring.pop() {
            let trb = unsafe {
                let trb = trb.into_event_trb().ok_or(XhciError::InvalidTrb);
                if trb.is_err() {
                    println!("Invalid event trb: {trb:?}");
                }
                trb.unwrap()
            };

            if let Err(e) = self.process_event(trb) {
                println!("Error while processing events: {e:?}");
            }
        }

        erdp.set_event_ring_dequeue_pointer(
            event_ring.deque_index() as u64 * size_of::<UnknownTRB>() as u64
                + event_ring.get_buf_ptr(),
        );
        erdp.clear_event_handler_busy();

        INT_REGS
            .lock()
            .get_mut()
            .interrupter_mut(0)
            .erdp
            .write_volatile(erdp);
    }

    fn process_event(&self, trb: trb::event::Allowed) -> Result<(), XhciError> {
        // println!("process_event: {trb:?}");
        match trb {
            trb::event::Allowed::TransferEvent(trb) => {
                self.trf_rings
                    .lock()
                    .get_mut(&(trb.slot_id() as usize))
                    .unwrap()
                    .get_mut(&(trb.endpoint_id() as usize))
                    .unwrap()
                    .set_deque_ptr(trb.trb_pointer());
                let sender = self.trb_callbacks.lock().remove(&trb.trb_pointer());
                if let Some(sender) = sender {
                    let trf_result = match trb.completion_code() {
                        Ok(CompletionCode::Success) | Ok(CompletionCode::ShortPacket) => Ok(trb),
                        _ => Err(trb),
                    };
                    let _ = sender.send(trf_result);
                }
            }
            trb::event::Allowed::CommandCompletion(trb) => {
                self.cmd_ring
                    .lock()
                    .set_deque_ptr(trb.command_trb_pointer());
                let sender = self
                    .cmd_callbacks
                    .lock()
                    .remove(&trb.command_trb_pointer())
                    .ok_or(XhciError::InvalidCommandCompletionTrb)?;
                let _ = sender.send(trb);
            }
            trb::event::Allowed::PortStatusChange(trb) => {
                let port_id = (trb.port_id() - 1) as usize;

                let portsc = XHCI_REGS
                    .lock()
                    .port_register_set
                    .read_volatile_at(port_id)
                    .portsc;
                if portsc.connect_status_change() && portsc.current_connect_status() {
                    update_port(port_id, |p| {
                        let p = &mut p.portsc;
                        p.set_0_connect_status_change();
                        p.set_0_over_current_change();
                        p.set_0_port_config_error_change();
                        p.set_0_port_enabled_disabled();
                        p.set_0_port_enabled_disabled_change();
                        p.set_0_port_link_state_change();
                        p.set_0_port_reset_change();
                        p.set_0_warm_port_reset_change();

                        p.clear_connect_status_change();
                    });
                    self.new_connection_callback.lock().send(port_id);
                } else if portsc.port_reset_change() {
                    update_port(port_id, |p| {
                        let p = &mut p.portsc;
                        p.set_0_connect_status_change();
                        p.set_0_over_current_change();
                        p.set_0_port_config_error_change();
                        p.set_0_port_enabled_disabled();
                        p.set_0_port_enabled_disabled_change();
                        p.set_0_port_link_state_change();
                        p.set_0_port_reset_change();
                        p.set_0_warm_port_reset_change();

                        p.clear_port_reset_change();
                    });
                    if let Some(sender) = self.port_reset_callbacks.lock().remove(&port_id) {
                        sender.send(trb).unwrap();
                    }
                }
            }
            trb::event::Allowed::BandwidthRequest(_) => todo!(),
            trb::event::Allowed::Doorbell(_) => todo!(),
            trb::event::Allowed::HostController(_) => todo!(),
            trb::event::Allowed::DeviceNotification(_) => todo!(),
            trb::event::Allowed::MfindexWrap(_) => todo!(),
        }

        Ok(())
    }

    pub fn push_cmd_trb(&self, trb: UnknownTRB) -> Result<u64, XhciError> {
        println!("{:?}", unsafe { trb.into_cmd_trb().unwrap() });
        let ptr = self.cmd_ring.lock().push(trb)?;
        ring_doorbell(0, 0);
        Ok(ptr as u64)
    }

    pub async fn wait_cmd_completion(&self, trb: u64) -> CommandCompletion {
        let (send, recv) = oneshot::channel();
        self.cmd_callbacks.lock().insert(trb, send);
        recv.await.unwrap()
    }

    pub async fn emit_command_async(
        &self,
        trb: UnknownTRB,
    ) -> Result<CommandCompletion, XhciError> {
        let trb_ptr = self.push_cmd_trb(trb)?;
        let result = self.wait_cmd_completion(trb_ptr).await;
        Ok(result)
    }

    pub async fn wait_port_reset(&self, port_id: usize) -> PortStatusChange {
        let (send, recv) = oneshot::channel();
        self.port_reset_callbacks.lock().insert(port_id, send);
        recv.await.unwrap()
    }

    pub async fn reset_port_async(&self, port_id: usize) {
        let portsc = read_port(port_id).portsc;
        println!(
            "resetting port {port_id}(CCS={}, CSC={})",
            portsc.current_connect_status(),
            portsc.connect_status_change()
        );

        update_port(port_id, |p| {
            let p = &mut p.portsc;
            p.set_0_connect_status_change();
            p.set_0_over_current_change();
            p.set_0_port_config_error_change();
            p.set_0_port_enabled_disabled();
            p.set_0_port_enabled_disabled_change();
            p.set_0_port_link_state_change();
            p.set_0_port_reset_change();
            p.set_0_warm_port_reset_change();

            p.set_port_reset();
        });

        self.wait_port_reset(port_id).await;
    }

    pub async fn enable_slot_async(&self) -> Result<usize, XhciError> {
        let trb_ptr = self.push_cmd_trb(unsafe { transmute(EnableSlot::new()) })?;
        let result = self.wait_cmd_completion(trb_ptr).await;
        Ok(result.slot_id() as usize)
    }

    fn get_ring_ptr(&self, slot_id: usize, endpoint_id: usize) -> Option<u64> {
        self.trf_rings
            .lock()
            .get(&slot_id)
            .and_then(|r| r.get(&endpoint_id))
            .map(ProducerRing::get_buf_ptr)
    }

    fn prepare_input_ctx_for_address_device(
        &self,
        port_id: usize,
        slot_id: usize,
        deque_ptr: u64,
    ) -> InputContext {
        /* 4.3.3 Device Slot Initialization */
        let mut input_ctx = InputContext::new(self.ctx_size);

        {
            let control = input_ctx.handler_mut().control_mut();
            control.set_add_context_flag(0);
            control.set_add_context_flag(1);
        }
        Self::config_slot_context(input_ctx.handler_mut().device_mut().slot_mut(), port_id);
        Self::config_default_control_pipe(
            input_ctx.handler_mut().device_mut().endpoint_mut(1),
            port_id,
            deque_ptr,
        );

        input_ctx
    }

    async fn address_device_async(
        &self,
        port_id: usize,
        slot_id: usize,
        bsr: bool,
    ) -> Result<(), XhciError> {
        self.init_device_at(port_id, slot_id);
        let trf_ring_ptr = self.init_trf_ring(slot_id, 1);

        let input_ctx = self.prepare_input_ctx_for_address_device(port_id, slot_id, trf_ring_ptr);

        let mut trb = AddressDevice::new();
        trb.set_input_context_pointer(input_ctx.get_address())
            .set_slot_id(slot_id as u8);
        if bsr {
            trb.set_block_set_address_request();
        }

        let result = self.emit_command_async(unsafe { transmute(trb) }).await?;

        println!("{:?}", input_ctx);
        self.with_device_at(slot_id, |d| println!("{:?}", d.out_ctx));

        let success = result
            .completion_code()
            .map_or(false, |code| matches!(code, CompletionCode::Success));

        if success {
            drop(input_ctx);
            Ok(())
        } else {
            Err(XhciError::AddressDeviceCommandFailed(result))
        }
    }

    fn config_slot_context(slot: &mut dyn SlotHandler, port_id: usize) {
        let speed = read_port(port_id).portsc.port_speed();
        slot.set_root_hub_port_number(port_id as u8 + 1);
        slot.set_route_string(0);
        slot.set_context_entries(1);
        slot.set_speed(speed);
    }

    fn config_default_control_pipe(
        pipe: &mut dyn EndpointHandler,
        port_id: usize,
        tr_deque_ptr: u64,
    ) {
        let speed = read_port(port_id).portsc.port_speed();
        let max_packet_size = match speed {
            1 => 64,  // full-speed
            2 => 8,   // Low-speed
            3 => 64,  // High-speed
            4 => 512, //SuperSpeed
            _ => 8,
        };

        pipe.set_endpoint_type(xhci::context::EndpointType::Control);
        pipe.set_max_packet_size(max_packet_size);
        pipe.set_max_burst_size(0);

        // xhci crate の仕様上、tr_deque_pointer を deque_cycle_state より先に設定する必要がある
        pipe.set_tr_dequeue_pointer(tr_deque_ptr);
        pipe.set_dequeue_cycle_state();

        pipe.set_interval(0);
        pipe.set_max_primary_streams(0);
        pipe.set_mult(0);
        pipe.set_error_count(3);
    }

    async fn init_device_exclusive_async(&self, port_id: usize) -> Result<(), XhciError> {
        println!("Addressing device at port={port_id}");

        self.port_status.lock()[port_id] = PortStatus::Connected;
        let addressing_port = self.addressing_port.lock().clone();
        if let Some(recv) = addressing_port {
            recv.await;
        }

        let (recv, send) = new_broadcast_channel();
        *self.addressing_port.lock() = Some(recv);

        self.reset_port_async(port_id).await;

        let slot_id = self.enable_slot_async().await?;

        self.address_device_async(port_id, slot_id, false).await?;
        // wait_for(200);

        println!("Addressing finished: port={port_id}, slot={slot_id}");

        send.send();
        self.notify_address_device_completion(slot_id);

        Ok(())
    }

    pub fn control_request(
        &self,
        slot_id: usize,
        setup: SetupData,
        data: Option<&mut [u8]>,
    ) -> Result<OneshotReceiver<Result<TransferEvent, TransferEvent>>, XhciError> {
        let (req_type, req) = setup.request_type.get_actual_value();

        let direction_bit = match req_type >> 7 == 1 {
            true => TransferDirection::DeviceToHost,
            false => TransferDirection::HostToDevice,
        };

        let (setup_transfer_type, data_dir, status_dir) = match (direction_bit, setup.length) {
            (TransferDirection::HostToDevice, 0) => (
                TransferType::No,
                Direction::Out,
                TransferDirection::DeviceToHost,
            ),
            (TransferDirection::HostToDevice, _) => (
                TransferType::Out,
                Direction::Out,
                TransferDirection::DeviceToHost,
            ),
            (TransferDirection::DeviceToHost, 0) => (
                TransferType::No,
                Direction::In,
                TransferDirection::DeviceToHost,
            ),
            (TransferDirection::DeviceToHost, _) => (
                TransferType::In,
                Direction::In,
                TransferDirection::HostToDevice,
            ),
        };

        let mut setup_trb = SetupStage::new();
        setup_trb
            .set_request_type(req_type)
            .set_request(req)
            .set_value(setup.value)
            .set_index(setup.index)
            .set_transfer_type(setup_transfer_type)
            .set_length(setup.length)
            .set_interrupt_on_completion();

        let mut status_trb = StatusStage::new();
        status_trb.set_interrupt_on_completion();
        if matches!(status_dir, TransferDirection::DeviceToHost) {
            status_trb.set_direction();
        }

        if setup.length == 0 {
            let trb = self
                .push_transfer_trb_async(slot_id, 1, transfer::Allowed::SetupStage(setup_trb))?
                .unwrap();
            self.push_transfer_trb_async(slot_id, 1, transfer::Allowed::StatusStage(status_trb))?;

            ring_doorbell(slot_id, 1);

            Ok(trb)
        } else {
            let mut data_trb = DataStage::new();
            data_trb
                .set_data_buffer_pointer(data.unwrap().as_mut_ptr() as u64)
                .set_trb_transfer_length(setup.length as u32)
                .set_td_size(0)
                .set_direction(data_dir)
                .set_interrupt_on_short_packet()
                .set_interrupt_on_completion();

            self.push_transfer_trb_async(slot_id, 1, transfer::Allowed::SetupStage(setup_trb))?;
            let trb = self
                .push_transfer_trb_async(slot_id, 1, transfer::Allowed::DataStage(data_trb))?
                .unwrap();
            self.push_transfer_trb_async(slot_id, 1, transfer::Allowed::StatusStage(status_trb))?;

            ring_doorbell(slot_id, 1);

            Ok(trb)
        }
    }

    fn push_transfer_trb_async(
        &self,
        slot_id: usize,
        endpoint_id: usize,
        trb: transfer::Allowed,
    ) -> Result<Option<OneshotReceiver<Result<TransferEvent, TransferEvent>>>, XhciError> {
        let mut trf_rings = self.trf_rings.lock();
        let trf_ring = trf_rings
            .get_mut(&slot_id)
            .unwrap()
            .get_mut(&endpoint_id)
            .unwrap();
        // println!("{:?}", trb);
        let ptr = trf_ring.push(unsafe { transmute(trb.into_raw()) })?;

        let int_on_short_packet = if let transfer::Allowed::DataStage(trb) = trb {
            trb.interrupt_on_short_packet()
        } else {
            false
        };

        if trb.interrupt_on_completion() || int_on_short_packet {
            let (sender, receiver) = channel();
            self.trb_callbacks.lock().insert(ptr as u64, sender);
            Ok(Some(receiver))
        } else {
            Ok(None)
        }
    }

    pub fn init_trf_ring(&self, slot_id: usize, endpoint_id: usize) -> u64 {
        let mut trf_rings = self.trf_rings.lock();
        let ring = ProducerRing::new(32);
        let ptr = ring.get_buf_ptr();
        trf_rings
            .entry(slot_id)
            .or_default()
            .insert(endpoint_id, ring);

        ptr
    }

    fn init_device_at(&self, port_id: usize, slot_id: usize) {
        let mut devices = self.devices.lock();
        let out_ctx = DeviceContext::new(self.context_size());
        devices.insert(
            slot_id,
            Device {
                port_id,
                slot_id,
                out_ctx,
            },
        );
        self.dcbaa.lock()[slot_id] = devices.get(&slot_id).unwrap().out_ctx.get_address();
    }

    pub fn with_device_at<R>(&self, slot_id: usize, f: impl FnOnce(&Device) -> R) -> R {
        f(self.devices.lock().get(&slot_id).unwrap())
    }

    pub fn context_size(&self) -> ContextSize {
        self.ctx_size
    }

    fn notify_address_device_completion(&self, slot_id: usize) {
        self.address_device_callback.lock().send(slot_id);
    }
}

bitfield! {
    #[derive(Clone,Copy)]
    #[repr(C, align(16))]
    pub struct UnknownTRB_ ([u64]);
    u8;
    pub cycle_bit, set_cycle_bit: 96;
    pub trb_type, _: 111,106;
}
pub(super) type UnknownTRB = UnknownTRB_<[u64; 2]>;

impl Default for UnknownTRB {
    fn default() -> Self {
        UnknownTRB_([0; 2])
    }
}

macro_rules! match_trb {
    ($type: ident, $value: ident, $($fr: path => $to: path),+) => {
        match $type {
            $(
                $fr => Some($to(transmute($value))),
            )+
            _ => None
        }
    }
}

impl UnknownTRB {
    pub unsafe fn into_cmd_trb(self: UnknownTRB) -> Option<trb::command::Allowed> {
        let trb_type: trb::Type = trb::Type::from_u8(self.trb_type())?;
        match_trb!(
            trb_type, self,
            Type::AddressDevice => trb::command::Allowed::AddressDevice,
            Type::Link => trb::command::Allowed::Link,
            Type::EnableSlot => trb::command::Allowed::EnableSlot,
            Type::DisableSlot => trb::command::Allowed::DisableSlot,
            Type::ConfigureEndpoint => trb::command::Allowed::ConfigureEndpoint,
            Type::EvaluateContext => trb::command::Allowed::EvaluateContext,
            Type::ResetEndpoint => trb::command::Allowed::ResetEndpoint,
            Type::StopEndpoint => trb::command::Allowed::StopEndpoint,
            Type::SetTrDequeuePointer => trb::command::Allowed::SetTrDequeuePointer,
            Type::ResetDevice => trb::command::Allowed::ResetDevice,
            Type::ForceEvent => trb::command::Allowed::ForceEvent,
            Type::NegotiateBandwidth => trb::command::Allowed::NegotiateBandwidth,
            Type::SetLatencyToleranceValue => trb::command::Allowed::SetLatencyToleranceValue,
            Type::GetPortBandwidth => trb::command::Allowed::GetPortBandwidth,
            Type::ForceHeader => trb::command::Allowed::ForceHeader,
            Type::NoopCommand => trb::command::Allowed::Noop,
            Type::GetExtendedProperty => trb::command::Allowed::GetExtendedProperty,
            Type::SetExtendedProperty => trb::command::Allowed::SetExtendedProperty
        )
    }

    pub unsafe fn into_trans_trb(self: UnknownTRB) -> Option<trb::transfer::Allowed> {
        let trb_type: trb::Type = trb::Type::from_u8(self.trb_type())?;
        match_trb!(
            trb_type, self,
            Type::Normal => trb::transfer::Allowed::Normal,
            Type::SetupStage => trb::transfer::Allowed::SetupStage,
            Type::DataStage => trb::transfer::Allowed::DataStage,
            Type::StatusStage => trb::transfer::Allowed::StatusStage,
            Type::Isoch => trb::transfer::Allowed::Isoch,
            Type::Link => trb::transfer::Allowed::Link,
            Type::EventData => trb::transfer::Allowed::EventData,
            Type::NoopTransfer => trb::transfer::Allowed::Noop
        )
    }

    pub unsafe fn into_event_trb(self: UnknownTRB) -> Option<trb::event::Allowed> {
        let trb_type: trb::Type = trb::Type::from_u8(self.trb_type())?;
        match_trb!(
            trb_type, self,
            Type::TransferEvent => trb::event::Allowed::TransferEvent,
            Type::CommandCompletion => trb::event::Allowed::CommandCompletion,
            Type::PortStatusChange => trb::event::Allowed::PortStatusChange,
            Type::BandwidthRequest => trb::event::Allowed::BandwidthRequest,
            Type::Doorbell => trb::event::Allowed::Doorbell,
            Type::HostController => trb::event::Allowed::HostController,
            Type::DeviceNotification => trb::event::Allowed::DeviceNotification,
            Type::MfindexWrap => trb::event::Allowed::MfindexWrap
        )
    }
}

#[derive(Clone)]
pub struct LinearMapper {}

impl Mapper for LinearMapper {
    unsafe fn map(&mut self, phys_start: usize, bytes: usize) -> core::num::NonZeroUsize {
        core::num::NonZeroUsize::new(phys_start).unwrap()
    }

    fn unmap(&mut self, virt_start: usize, bytes: usize) {}
}
