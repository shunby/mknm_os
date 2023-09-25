use futures::channel::oneshot;
use xhci::ring::trb::{
    self,
    transfer::{self, Normal},
};

use crate::usb::{
    usbd::{Descriptor, UsbInterfaceAlternate},
    xhci::{
        push_transfer_trb_async, ring_doorbell, ControlRequestType, SetupData, XhciError, XHCI,
    },
};

use alloc::boxed::Box;

#[repr(C)]
#[derive(Debug, Default, Clone)]
pub struct MouseReport {
    buttons: u8,
    dx: i8,
    dy: i8,
}

impl MouseReport {
    pub fn dx(&self) -> i8 {
        self.dx
    }

    pub fn dy(&self) -> i8 {
        self.dy
    }

    pub fn buttons(&self) -> u8 {
        self.buttons
    }
}

pub struct MouseClass {
    slot_id: usize,
    interface: u8,
    dci: usize,
}

impl MouseClass {
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
        let recv = XHCI.lock().control_request(self.slot_id, setup, None)?;
        recv.await.unwrap().map_err(XhciError::TransferError)?;

        Ok(())
    }

    pub fn subscribe_once(
        &self,
    ) -> Result<
        (
            oneshot::Receiver<Result<trb::event::TransferEvent, trb::event::TransferEvent>>,
            Box<MouseReport>,
        ),
        XhciError,
    > {
        let mut trb = Normal::new();
        let buf: Box<MouseReport> = Box::default();
        trb.set_interrupt_on_completion()
            .set_data_buffer_pointer(buf.as_ref() as *const MouseReport as u64)
            .set_trb_transfer_length(8);
        let recv = push_transfer_trb_async(self.slot_id, self.dci, transfer::Allowed::Normal(trb))?
            .unwrap();
        ring_doorbell(self.slot_id, self.dci as u8);
        Ok((recv, buf))
    }
}
