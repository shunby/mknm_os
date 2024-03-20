use super::ring::ProducerRing;
use crate::usb::xhci::{LinearMapper, UnknownTRB_, XhciError};
use alloc::collections::BTreeMap;
use futures::channel::oneshot;
use xhci::{ring::trb::{self, event::CommandCompletion}, Registers};

/// Command Ringの管理
/// リングへのTRB追加、CommandCompletionのリスナーへの通知
pub struct CommandRing {
    ring: ProducerRing,
    listener: BTreeMap<u64, oneshot::Sender<CommandCompletion>>,
}

pub fn init_command_ring(size: usize, regs: &mut Registers<LinearMapper>) -> CommandRing {
    let ring = ProducerRing::new(32);
    regs.operational.crcr.update_volatile(|x| {
        x.set_command_ring_pointer(ring.get_buf_ptr());
        x.set_ring_cycle_state();
    });

    CommandRing {
        ring,
        listener: BTreeMap::new()
    }
}

impl CommandRing {

    pub fn push_command(&mut self, trb: trb::command::Allowed, regs: &mut Registers<LinearMapper>) -> Result<oneshot::Receiver<CommandCompletion>, XhciError> {
        let ptr = self.ring.push(UnknownTRB_(trb.into_raw()))? as u64;
        
        regs.doorbell.update_volatile_at(0, |d|{
            d.set_doorbell_target(0);
        });
        
        let (send, recv) = oneshot::channel();
        self.listener.insert(ptr, send);
        Ok(recv)
    }

    pub async fn emit_command_async(&mut self, trb: trb::command::Allowed, regs: &mut Registers<LinearMapper>) -> Result<CommandCompletion, XhciError> {
        Ok(self.push_command(trb, regs)?.await.unwrap())
    }

    pub fn on_command_completion(&mut self, completion: CommandCompletion) {
        if let Some(rcv) = self.listener.remove(&completion.command_trb_pointer()) {
            rcv.send(completion);
        }
    }
}