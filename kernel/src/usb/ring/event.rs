use core::mem::{size_of, ManuallyDrop};

use bitfield::bitfield;
use xhci::{ring::trb::{self, event::{CommandCompletion, PortStatusChange, TransferEvent}}, Registers};

use super::ring::ConsumerRing;
use crate::usb::{xhci::{LinearMapper, UnknownTRB}, runtime::Sender};

/// XHCからの割り込みを受けて、EventRingに追加されたイベントを確認、Listenerに通知する
pub struct EventRing {
    ring: ConsumerRing,
    trf_listener: Sender<TransferEvent>,
    cmd_listener: Sender<CommandCompletion>,
    port_listener: Sender<PortStatusChange>
}

bitfield! {
    #[repr(C)]
    struct EventRingSegmentTableEntry ([u64]);
    u64;
    base_addr, set_base_addr: 63,0;
    ring_segment_size, set_ring_segment_size: 79,64;
}

pub fn init_event_ring(regs: &mut Registers<LinearMapper>, trf_listener: Sender<TransferEvent>, cmd_listener: Sender<CommandCompletion>, port_listener: Sender<PortStatusChange>) -> EventRing{
    let ring = ConsumerRing::new(32);
    
    let mut entry = EventRingSegmentTableEntry([0; 2]);
    entry.set_base_addr(ring.get_buf_ptr());
    entry.set_ring_segment_size(ring.size() as u64);
    
    let er_table = ManuallyDrop::new(vec![entry]);

    let mut iregs = regs.interrupter_register_set.interrupter_mut(0);
    iregs
        .erstsz
        .update_volatile(|x| x.set(er_table.len() as u16));
    iregs
        .erstba
        .update_volatile(|x| x.set(er_table.as_ptr() as u64));
    iregs.erdp.update_volatile(|x| {
        x.set_0_event_handler_busy();
        x.set_event_ring_dequeue_pointer(ring.get_buf_ptr())
    });
    
    EventRing {
        ring,
        trf_listener,
        cmd_listener,
        port_listener
    }
}

impl EventRing {
    pub fn on_xhc_interrupt(&mut self, regs: &mut Registers<LinearMapper>) {
        while let Some(trb) = self.ring.pop() {
            let trb = unsafe {
                trb.into_event_trb().expect("Invalid event trb")
            };

            self.process_event(trb);
        }

        regs.interrupter_register_set.interrupter_mut(0).erdp.update_volatile(|x|{
            x.set_event_ring_dequeue_pointer(self.ring.deque_index() as u64 * size_of::<UnknownTRB>() as u64 + self.ring.get_buf_ptr());
            x.clear_event_handler_busy();
        });
    }

    fn process_event(&self, trb: trb::event::Allowed) {
        // println!("process_event: {trb:?}");
        match trb {
            trb::event::Allowed::TransferEvent(trb) => {
                self.trf_listener.send(trb);
            },
            trb::event::Allowed::CommandCompletion(trb) => {
                self.cmd_listener.send(trb);
            },
            trb::event::Allowed::PortStatusChange(trb) => {
                self.port_listener.send(trb);
            },
            trb::event::Allowed::BandwidthRequest(_) => todo!(),
            trb::event::Allowed::Doorbell(_) => todo!(),
            trb::event::Allowed::HostController(_) => todo!(),
            trb::event::Allowed::DeviceNotification(_) => todo!(),
            trb::event::Allowed::MfindexWrap(_) => todo!(),
        }
    }
}
