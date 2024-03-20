use core::{
    alloc::{AllocError, Allocator, Layout},
    mem::transmute,
    ptr::{read_volatile, write_volatile, NonNull},
};

use alloc::
    alloc::Global
;
use bitfield::bitfield;
use futures::channel::oneshot;
use num_traits::cast::FromPrimitive;
use xhci::{
    accessor::Mapper,
    ring::trb,
    ring::trb::{
        event::{CommandCompletion, TransferEvent},
        Type,
    },
    Registers,
};

use crate::{
    memory_manager::LazyInit, pci::PCIDevice, println, usb::{
        action::init_device::DeviceInitAction, device::init_dcbaa, ring::{command::init_command_ring, event::init_event_ring, transfer::TransferRingSet}, runtime::new_channel
    }
};

use super::{
    device::Dcbaa, ring::{command::CommandRing, event::EventRing, transfer::SetupData}, runtime::{Sender, Spawner}, 
};

static EVENT_RING: LazyInit<EventRing> = LazyInit::new();
static CMD_RING: LazyInit<CommandRing> = LazyInit::new();
static TRF_RINGS: LazyInit<TransferRingSet> = LazyInit::new();
static DCBAA: LazyInit<Dcbaa> = LazyInit::new();
static REGS: LazyInit<Registers<LinearMapper>> = LazyInit::new();

#[derive(Debug)]
pub enum XhciError {
    InvalidTrb,
    RingIsFull,
    InvalidCommandCompletionTrb,
    AddressDeviceCommandFailed(CommandCompletion),
    UnexpectedDescriptor,
    TransferError(TransferEvent),
}

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

pub fn push_command(trb: trb::command::Allowed) -> Result<oneshot::Receiver<CommandCompletion>, XhciError> {
    CMD_RING.lock().push_command(trb, &mut REGS.lock())
}

pub fn push_transfer_trb(
    slot_id: usize,
    endpoint_id: usize,
    trb: trb::transfer::Allowed,
) -> Result<Option<oneshot::Receiver<Result<TransferEvent, XhciError>>>, XhciError> {
    TRF_RINGS.lock().push_transfer_trb(slot_id, endpoint_id, trb)
}

pub fn control_request(
    slot_id: usize,
    setup: SetupData,
    data: Option<&mut [u8]>,
) -> Result<oneshot::Receiver<Result<TransferEvent, XhciError>>, XhciError> {
    TRF_RINGS.lock().control_request(slot_id, setup, data, &mut REGS.lock())
}

pub fn on_xhc_interrupt() {
    EVENT_RING.lock().on_xhc_interrupt(&mut REGS.lock());
}

pub fn with_regs<R>(f: impl FnOnce(&mut Registers<LinearMapper>)->R) -> R {
    f(&mut REGS.lock())
}

pub fn with_dcbaa<R>(f: impl FnOnce(&mut Dcbaa)->R) -> R {
    f(&mut DCBAA.lock())
}

pub fn with_trf_rings<R>(f: impl FnOnce(&mut TransferRingSet)->R) -> R {
    f(&mut TRF_RINGS.lock())
}

pub unsafe fn initialize_xhci(
    xhc: PCIDevice,
    intel_ehci_found: bool,
    spawner: &mut Spawner<'static, Result<(), XhciError>>,
    addr_send: Sender<usize>
)
{
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
    reset_hc(&mut regs);

    let num_ports = regs.capability.hcsparams1.read_volatile().number_of_ports();
    let mut dcbaa = init_dcbaa(&mut regs);
    
    let (cmd_send, cmd_recv) = new_channel();
    let (trf_send, trf_recv) = new_channel();
    let (port_send, port_recv) = new_channel();
    
    let cmd_ring = init_command_ring(32, &mut regs);
    let event_ring = init_event_ring(&mut regs, trf_send, cmd_send, port_send);

    enable_xhci_interrupt_and_start(&mut regs);

    EVENT_RING.lock().init(event_ring);
    CMD_RING.lock().init(cmd_ring);
    TRF_RINGS.lock().init(TransferRingSet::new(32));
    DCBAA.lock().init(dcbaa);
    REGS.lock().init(regs);

    spawner.spawn(async move {
        loop {
            let completion = cmd_recv.receive_async().await;
            CMD_RING.lock().on_command_completion(completion);
        }
    });

    
    spawner.spawn(async move {
        loop {
            let trf_evt = trf_recv.receive_async().await;
            TRF_RINGS.lock().on_trf_event(trf_evt);
        }
    });

    spawner.spawn(async move {
        let mut device_initializer = DeviceInitAction::new(port_recv, addr_send);
        device_initializer.main_loop().await;
        Ok(())
    });

    // let mut usbd = UsbDriver::new(addr_receiver, Box::new(mouse_callback));

    // println!("xHCI initialization complete");
    // run_xhci_tasks();
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

unsafe fn reset_hc(regs: &mut Registers<LinearMapper>) {
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
}

fn enable_xhci_interrupt_and_start(regs: &mut Registers<LinearMapper>) {
    let mut iregs = regs.interrupter_register_set.interrupter_mut(0);
    iregs.imod.update_volatile(|x| {
        x.set_interrupt_moderation_interval(4000);
    });
    iregs.iman.update_volatile(|x| {
        x.clear_interrupt_pending();
        x.set_interrupt_enable();
    });

    regs.operational.usbcmd.update_volatile(|x| {
        x.set_interrupter_enable();
    });

    regs.operational.usbcmd.update_volatile(|x| {
        x.set_run_stop();
    });

    while regs.operational.usbsts.read_volatile().hc_halted() {}
}

bitfield! {
    #[derive(Clone,Copy)]
    #[repr(C, align(16))]
    pub struct UnknownTRB_ ([u32]);
    u8;
    pub cycle_bit, set_cycle_bit: 96;
    pub trb_type, _: 111,106;
}
pub(super) type UnknownTRB = UnknownTRB_<[u32; 4]>;

impl Default for UnknownTRB {
    fn default() -> Self {
        UnknownTRB_([0; 4])
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

pub struct AlignedAlloc<const N: usize> {}

unsafe impl<const N: usize> Allocator for AlignedAlloc<N> {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        Global.allocate(layout.align_to(N).unwrap())
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        Global.deallocate(ptr, layout.align_to(N).unwrap())
    }
}
