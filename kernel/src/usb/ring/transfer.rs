use super::ring::ProducerRing;
use crate::usb::xhci::{LinearMapper, UnknownTRB_, XhciError};
use alloc::collections::BTreeMap;
use futures::channel::oneshot;
use xhci::{ring::trb::{self, event::{CompletionCode, TransferEvent}, transfer::{Allowed, DataStage, Direction, SetupStage, StatusStage, TransferType}}, Registers};

pub struct TransferRingSet {
    rings: BTreeMap<(usize, usize), ProducerRing>,
    listener: BTreeMap<u64, oneshot::Sender<Result<TransferEvent, XhciError>>>,
    ring_size: usize
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

impl TransferRingSet {
    pub fn new(ring_size: usize) -> Self {
        Self {
            rings: BTreeMap::new(),
            listener: BTreeMap::new(),
            ring_size
        }
    }

    pub fn on_trf_event(&mut self, evt: TransferEvent) {
        self.rings.get_mut(&(evt.slot_id() as usize, evt.endpoint_id() as usize))
                    .unwrap()
                    .set_deque_ptr(evt.trb_pointer());
        let result = match evt.completion_code() {
            Ok(CompletionCode::Success | CompletionCode::ShortPacket) => Ok(evt),
            _ => Err(XhciError::TransferError(evt))
        };
        
        if let Some(rcv) = self.listener.remove(&evt.trb_pointer()) {
            let _ = rcv.send(result);
        }
    }

    pub fn init_ring_at(&mut self, slot_id: usize, endpoint_id: usize) -> u64{
        self.rings.insert((slot_id, endpoint_id), ProducerRing::new(self.ring_size));
        self.rings[&(slot_id, endpoint_id)].get_buf_ptr()
    }

    
    pub fn control_request(
        &mut self,
        slot_id: usize,
        setup: SetupData,
        data: Option<&mut [u8]>,
        regs: &mut Registers<LinearMapper>
    ) -> Result<oneshot::Receiver<Result<TransferEvent, XhciError>>, XhciError> {
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
                .push_transfer_trb(slot_id, 1, Allowed::SetupStage(setup_trb))?
                .unwrap();
            self.push_transfer_trb(slot_id, 1, Allowed::StatusStage(status_trb))?;

            regs.doorbell.update_volatile_at(slot_id, |d|{d.set_doorbell_target(1);});

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

            self.push_transfer_trb(slot_id, 1, Allowed::SetupStage(setup_trb))?;
            let trb = self
                .push_transfer_trb(slot_id, 1, Allowed::DataStage(data_trb))?
                .unwrap();
            self.push_transfer_trb(slot_id, 1, Allowed::StatusStage(status_trb))?;

            regs.doorbell.update_volatile_at(slot_id, |d|{d.set_doorbell_target(1);});

            Ok(trb)
        }
    }

    
    pub fn push_transfer_trb(
        &mut self,
        slot_id: usize,
        endpoint_id: usize,
        trb: trb::transfer::Allowed,
    ) -> Result<Option<oneshot::Receiver<Result<TransferEvent, XhciError>>>, XhciError> {
        let trf_ring = self.rings.get_mut(&(slot_id, endpoint_id)).unwrap();
        // println!("{:?}", trb);
        let ptr = trf_ring.push(UnknownTRB_(trb.into_raw()))?;

        let int_on_short_packet = if let trb::transfer::Allowed::DataStage(trb) = trb {
            trb.interrupt_on_short_packet()
        } else {
            false
        };

        if trb.interrupt_on_completion() || int_on_short_packet {
            let (sender, receiver) = oneshot::channel();
            self.listener.insert(ptr as u64, sender);
            Ok(Some(receiver))
        } else {
            Ok(None)
        }
    }
}