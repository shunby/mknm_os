use core::{mem::{MaybeUninit, transmute, size_of}, default, iter::repeat_with};

use alloc::{boxed::Box, vec::Vec};
use bitfield::bitfield;
use usb_bindings::raw::usb_set_default_mouse_observer;
use xhci::{accessor::Mapper, registers::{operational::{UsbStatusRegister, UsbCommandRegister}, InterrupterRegisterSet, runtime::EventRingSegmentTableSizeRegister, PortRegisterSet}, ring::trb::{self, Link}, ring::trb::{Type, command::EnableSlot}, Registers};
use num_traits::cast::FromPrimitive;

static mut MOUSE_OBSERVER: MaybeUninit<Box<dyn Fn(i8,i8)>> = MaybeUninit::uninit();

pub unsafe fn set_default_mouse_observer(f: impl Fn(i8, i8) + 'static) {
    MOUSE_OBSERVER = MaybeUninit::new(Box::new(f));
    usb_set_default_mouse_observer(Some(observer));
}

unsafe extern "C" fn observer(x: i8, y: i8) {
    MOUSE_OBSERVER.assume_init_ref()(x,y);
}

pub enum XhciError {
    UnknownTrbType,
    InvalidTrb,
    RingIsFull,
    RingIsEmpty,
    InvalidPortStatus,
    MustWaitForAddress,
    ConfQueueIsEmpty,
    InvalidCompletionCode(u8),
}

pub unsafe fn initialize_xhci(mmio_base: usize) {
    let regs = xhci::Registers::new(mmio_base, LinearMapper {});
    let mut op = regs.operational;

    assert!(op.usbsts.read_volatile().hc_halted());
    op.usbcmd.update_volatile(|x|{x.set_host_controller_reset();});
    while op.usbcmd.read_volatile().host_controller_reset() {}
    while op.usbsts.read_volatile().controller_not_ready() {}

    let max_slots = regs.capability.hcsparams1.read_volatile().number_of_device_slots();
    op.config.update_volatile(|cfg|{cfg.set_max_device_slots_enabled(max_slots);});

    let mut dcbaa = vec![0u8;max_slots as usize];
    op.dcbaap.update_volatile(|x|{x.set(dcbaa.as_mut_ptr() as u64)});

    let mut cmd_ring = ProducerRing::new(32);
    op.crcr.update_volatile(|x|{
        x.set_command_ring_pointer(cmd_ring.get_buf_ptr());
        x.set_ring_cycle_state();
    });

    let mut evt_ring = ConsumerRing::new(32);
    let mut entry = EventRingSegmentTableEntry([0;2]);
    entry.set_base_addr(evt_ring.get_buf_ptr());
    entry.set_ring_segment_size(evt_ring.size() as u64);
    let er_table : Vec<EventRingSegmentTableEntry<[u64;2]>> = vec![entry];
    let mut ints = InterrupterRegisterSet::new(mmio_base, regs.capability.rtsoff.read_volatile(), LinearMapper{});
    let mut iregs = ints.interrupter_mut(0);
    iregs.erstsz.update_volatile(|x|x.set(er_table.len() as u16));
    iregs.erstba.update_volatile(|x|x.set(er_table.as_ptr() as u64));
    iregs.erdp.update_volatile(|x|x.set_event_ring_dequeue_pointer(evt_ring.get_buf_ptr()));

    iregs.imod.update_volatile(|x|{x.set_interrupt_moderation_interval(4000);});
    iregs.iman.update_volatile(|x|{x.set_interrupt_enable(); x.clear_interrupt_pending();});
    op.usbcmd.update_volatile(|x|{x.set_interrupter_enable();});

    op.usbcmd.update_volatile(|x|{x.set_run_stop();});
    while op.usbsts.read_volatile().hc_halted() {}

}



bitfield!{
    struct EventRingSegmentTableEntry ([u64]);
    u64;
    base_addr, set_base_addr: 63,0;
    ring_segment_size, set_ring_segment_size: 79,64;
}

#[derive(PartialEq, Eq)]
enum PortStatus {
    Disconnected,
    Resetting,
    EnablingSlot,
    Enabled
}

pub struct XhciController {
    regs: Registers<LinearMapper>,
    ints: InterrupterRegisterSet<LinearMapper>,
    event_ring: ConsumerRing,
    cmd_ring: ProducerRing,
    port_status: Vec<PortStatus>,
    port_conf_queue: Vec<usize>,
}

impl XhciController {
    fn process_events(&mut self) -> Result<(), XhciError> {
        let mut erdp = self.ints.interrupter_mut(0).erdp.read_volatile();
        let deq_index = (erdp.event_ring_dequeue_pointer() - self.event_ring.get_buf_ptr()) / size_of::<UnknownTRB>() as u64;
        self.event_ring.set_deque_index(deq_index as usize);
        
        while let Some(trb) = self.event_ring.pop()? {
            let trb = unsafe { trb.into_event_trb().ok_or(XhciError::InvalidTrb)? };
            self.process_event(trb);
        }
        erdp.set_event_ring_dequeue_pointer(self.event_ring.deque_index() as u64 * size_of::<UnknownTRB>() as u64 + self.event_ring.get_buf_ptr());
        erdp.clear_event_handler_busy();

        Ok(())
    }

    #[inline]
    fn read_port(&self, port_id: usize) -> PortRegisterSet {
        self.regs.port_register_set.read_volatile_at(port_id)
    }

    #[inline]
    fn update_port<U: FnOnce(&mut PortRegisterSet)>(&mut self, port_id: usize, f: U) {
        self.regs.port_register_set.update_volatile_at(port_id, f);
    }

    fn try_reset_port(&mut self, port_id: usize) -> Result<(), XhciError> {
        let portsc = self.read_port(port_id).portsc;
        if !portsc.current_connect_status() || !portsc.connect_status_change() {
            return Err(XhciError::InvalidPortStatus);
        } else if !self.port_conf_queue.is_empty() {
            self.port_conf_queue.push(port_id);
            return Ok(());
        }

        self.update_port(port_id, |p|{
            p.portsc.clear_connect_status_change();
            p.portsc.set_port_reset();
        });

        self.port_status[port_id] = PortStatus::Resetting;
        self.port_conf_queue.push(port_id);

        Ok(())
    }

    fn enable_slot(&mut self, port_id: usize) -> Result<(), XhciError> {
        let portsc = self.read_port(port_id).portsc;
        if !portsc.port_enabled_disabled() || !portsc.port_reset_change() {
            return Err(XhciError::InvalidPortStatus);
        } else if self.port_conf_queue.is_empty() {
            return Err(XhciError::ConfQueueIsEmpty);
        } else if self.port_conf_queue[0] != port_id {
            return Err(XhciError::MustWaitForAddress);
        }
        
        self.cmd_ring.push(unsafe {
            transmute(EnableSlot::new())
        })?;

        self.update_port(port_id, |x|{
            x.portsc.clear_port_reset_change();
        });
        self.port_status[port_id] = PortStatus::EnablingSlot;

        Ok(())
    }

    fn address_device(&mut self, slot_id: u8) -> Result<(), XhciError> {
        if self.port_conf_queue.is_empty() {
            return Err(XhciError::ConfQueueIsEmpty);
        }
        let port_id = self.port_conf_queue[0];
        
        if self.port_status[port_id] != PortStatus::EnablingSlot {
            return Err(XhciError::InvalidPortStatus);
        }

        Ok(())
        

    }

    fn process_event(&mut self, trb: trb::event::Allowed) -> Result<(), XhciError> {
        match trb {
            trb::event::Allowed::TransferEvent(_) => todo!(),
            trb::event::Allowed::CommandCompletion(trb) => {
                let code = trb.completion_code().map_err(|code|XhciError::InvalidCompletionCode(code))?;
                let mut cmd_trb = unsafe{
                    let trb = *(trb.command_trb_pointer() as *mut UnknownTRB);
                    trb.into_cmd_trb().unwrap()
                };
                match cmd_trb {
                    trb::command::Allowed::EnableSlot(_) => {
                        self.address_device(trb.slot_id());
                    },
                    trb::command::Allowed::Link(_) => todo!(),
                    trb::command::Allowed::DisableSlot(_) => todo!(),
                    trb::command::Allowed::AddressDevice(_) => todo!(),
                    trb::command::Allowed::ConfigureEndpoint(_) => todo!(),
                    trb::command::Allowed::EvaluateContext(_) => todo!(),
                    trb::command::Allowed::ResetEndpoint(_) => todo!(),
                    trb::command::Allowed::StopEndpoint(_) => todo!(),
                    trb::command::Allowed::SetTrDequeuePointer(_) => todo!(),
                    trb::command::Allowed::ResetDevice(_) => todo!(),
                    trb::command::Allowed::ForceEvent(_) => todo!(),
                    trb::command::Allowed::NegotiateBandwidth(_) => todo!(),
                    trb::command::Allowed::SetLatencyToleranceValue(_) => todo!(),
                    trb::command::Allowed::GetPortBandwidth(_) => todo!(),
                    trb::command::Allowed::ForceHeader(_) => todo!(),
                    trb::command::Allowed::Noop(_) => todo!(),
                    trb::command::Allowed::GetExtendedProperty(_) => todo!(),
                    trb::command::Allowed::SetExtendedProperty(_) => todo!(),
                }
            },
            trb::event::Allowed::PortStatusChange(trb) => {
                let port_id = trb.port_id() as usize;
                match self.port_status[port_id] {
                    PortStatus::Disconnected => {
                        self.try_reset_port(port_id)?;
                    },
                    PortStatus::Resetting => {
                        self.enable_slot(port_id)?;
                    },
                    PortStatus::EnablingSlot => {
                        
                    },

                    PortStatus::Enabled => todo!(),
                }
            },
            trb::event::Allowed::BandwidthRequest(_) => todo!(),
            trb::event::Allowed::Doorbell(_) => todo!(),
            trb::event::Allowed::HostController(_) => todo!(),
            trb::event::Allowed::DeviceNotification(_) => todo!(),
            trb::event::Allowed::MfindexWrap(_) => todo!(),
        }

        Ok(())
    }
}

bitfield! {
    #[derive(Clone,Copy)]
    struct UnknownTRB_ ([u64]);
    u8;
    cycle_bit, set_cycle_bit: 96;
    trb_type, _: 111,106;
}
type UnknownTRB = UnknownTRB_<[u64;2]>;

impl Default for UnknownTRB {
    fn default() -> Self {
        UnknownTRB_([0;2])
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
    unsafe fn into_cmd_trb(self: UnknownTRB) -> Option<trb::command::Allowed> {
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

    unsafe fn into_trans_trb(self: UnknownTRB) -> Option<trb::transfer::Allowed> {
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
    

    unsafe fn into_event_trb(self: UnknownTRB) -> Option<trb::event::Allowed> {
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

pub struct ProducerRing {
    data: Vec<UnknownTRB>,
    cycle_state: bool,
    enque: usize,
    deque: usize,
}

impl ProducerRing {
    pub fn new(size: usize) -> Self {
        let mut data: Vec<UnknownTRB> = repeat_with(UnknownTRB::default).take(size).collect();
        data[size-1] = unsafe {
            let mut link = Link::new(); 
            link.set_ring_segment_pointer(data.as_ptr() as u64).set_toggle_cycle();
            transmute(link)
        };
        
        Self {
            data,
            cycle_state: true,
            enque: 0,
            deque: 0
        }
    }

    /**
     * self.enqueがLink TRB を示すことはない
     */
    pub fn push(&mut self, mut trb: UnknownTRB) -> Result<(), XhciError> {
        if self.enque + 1 == self.deque {
            return Err(XhciError::RingIsFull)
        }

        trb.set_cycle_bit(self.cycle_state);
        self.data[self.enque] = trb;
        
        self.enque += 1;
        if self.enque == self.data.len() {
            self.enque = 0;
            self.cycle_state = !self.cycle_state;
        }

        Ok(())
    }

    pub fn clear(&mut self) {
        self.deque = self.enque;
    }

    pub fn cycle_state(&self) -> bool {
        self.cycle_state
    }

    pub fn get_buf_ptr(&self) -> u64 {
        self.data.as_ptr() as u64
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }
}


pub struct ConsumerRing {
    data: Vec<UnknownTRB>,
    cycle_state: bool,
    deque: usize,
}

impl ConsumerRing {
    pub fn new(size: usize) -> Self {
        let mut data: Vec<UnknownTRB> = repeat_with(UnknownTRB::default).take(size).collect();
        data[size-1] = unsafe {
            let mut link = Link::new(); 
            link.set_ring_segment_pointer(data.as_ptr() as u64).set_toggle_cycle();
            transmute(link)
        };
        
        Self {
            data,
            cycle_state: true,
            deque: 0
        }
    }

    pub fn deque_index(&self) -> usize {
        self.deque
    }

    pub fn set_deque_index(&mut self, deque_index: usize){
        self.deque = deque_index;
    }

    pub fn pop(&mut self) -> Result<Option<UnknownTRB>, XhciError> {
        let trb = self.data[self.deque];
        
        if trb.cycle_bit() != self.cycle_state {
            return Err(XhciError::RingIsEmpty);
        }
        
        self.deque += 1;
        if self.deque == self.data.len() {
            self.deque = 0;
            self.cycle_state = !self.cycle_state;
        }

        Ok(Some(trb))
    }


    pub fn cycle_state(&self) -> bool {
        self.cycle_state
    }

    pub fn get_buf_ptr(&self) -> u64 {
        self.data.as_ptr() as u64
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }
}

#[derive(Clone)]
struct LinearMapper {

}

impl Mapper for LinearMapper {
    unsafe fn map(&mut self, phys_start: usize, bytes: usize) -> core::num::NonZeroUsize {
        core::num::NonZeroUsize::new(phys_start).unwrap()
    }

    fn unmap(&mut self, virt_start: usize, bytes: usize) {
        
    }
}