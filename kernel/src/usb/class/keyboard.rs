use futures::channel::oneshot;
use xhci::ring::trb::{
    self,
    transfer::{self, Normal},
};

use crate::usb::{
    ring::transfer::{ControlRequestType, SetupData}, usbd::{Descriptor, UsbInterfaceAlternate}, xhci::{control_request, push_transfer_trb, with_regs, XhciError}
};

use alloc::boxed::Box;

use super::key::ModifierSet;

#[repr(C)]
#[derive(Debug, Default, Clone)]
pub struct KeyReport {
    pub modifier: ModifierSet,
    pub _rsvd: u8,
    pub keycodes: [u8;6],
}

pub struct KeyboardClass {
    slot_id: usize,
    interface: u8,
    dci: usize,
}

impl KeyboardClass {
    pub fn new(slot_id: usize, interface: &UsbInterfaceAlternate) -> Option<Self> {
        let mut dci = None;
        for desc in interface.endpoints() {
            if let Descriptor::Endpoint(desc) = desc {
                dci = Some(desc.calc_dci());
                break;
            }
        }

        Some(Self {
            slot_id,
            interface: interface.interface_num(),
            dci: dci?,
        })
    }

    pub async fn initialize(&self) -> Result<(), XhciError> {
        /* set boot protocol */
        let setup = SetupData {
            request_type: ControlRequestType::SetProtocol,
            value: 0,
            index: self.interface as u16,
            length: 0,
        };
        control_request(self.slot_id, setup, None)?.await.unwrap()?;

        Ok(())
    }

    pub fn subscribe_once(
        &self,
    ) -> Result<
        (
            oneshot::Receiver<Result<trb::event::TransferEvent, XhciError>>,
            Box<KeyReport>,
        ),
        XhciError,
    > {
        let mut trb = Normal::new();
        let buf: Box<KeyReport> = Box::default();
        trb.set_interrupt_on_completion()
            .set_data_buffer_pointer(buf.as_ref() as *const KeyReport as u64)
            .set_trb_transfer_length(8);
        let recv = push_transfer_trb(self.slot_id, self.dci, transfer::Allowed::Normal(trb))?.unwrap();
        with_regs(|r|r.doorbell.update_volatile_at(self.slot_id, |d|{d.set_doorbell_target(self.dci as u8);}));
        Ok((recv, buf))
    }
}
