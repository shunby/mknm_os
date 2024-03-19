use core::{
    iter::repeat_with,
    mem::{size_of, transmute},
};

use xhci::ring::trb::Link;

use super::xhci::{UnknownTRB, XhciError};

use alloc::vec::Vec;

use alloc::string::ToString;

use alloc::boxed::Box;

use crate::{println, print};

pub struct ProducerRing {
    data: Box<[UnknownTRB]>,
    cycle_state: bool,
    enque: usize,
    deque: usize,
}

impl ProducerRing {
    pub fn new(size: usize) -> Self {
        let mut data = repeat_with(UnknownTRB::default)
            .take(size)
            .collect::<Vec<UnknownTRB>>()
            .into_boxed_slice();
        data[size - 1] = unsafe {
            let mut link = Link::new();
            link.set_ring_segment_pointer(data.as_ptr() as u64)
                .set_toggle_cycle();
            transmute(link)
        };

        Self {
            data,
            cycle_state: true,
            enque: 0,
            deque: 0,
        }
    }

    pub fn next_ptr(&mut self, ptr: usize) -> usize {
        debug_assert!(ptr <= self.data.len() - 2);
        if ptr + 1 == self.data.len() - 1 {
            0
        } else {
            ptr + 1
        }
    }

    fn advance_enque_ptr(&mut self) {
        self.enque += 1;
        if self.enque == self.data.len() - 1 {
            self.data[self.enque].set_cycle_bit(self.cycle_state);
            self.enque = 0;
            self.cycle_state = !self.cycle_state;
        }
    }

    pub fn push(&mut self, mut trb: UnknownTRB) -> Result<*mut UnknownTRB, XhciError> {
        if self.next_ptr(self.enque) == self.deque {
            return Err(XhciError::RingIsFull);
        }

        trb.set_cycle_bit(self.cycle_state);
        self.data[self.enque] = trb;
        let ret_ptr = &mut self.data[self.enque] as *mut UnknownTRB;

        self.advance_enque_ptr();

        Ok(ret_ptr)
    }

    pub fn set_deque_ptr(&mut self, deque_ptr: u64) {
        let index = (deque_ptr - self.get_buf_ptr()) as usize / size_of::<UnknownTRB>();
        self.deque = self.next_ptr(index);
    }

    pub fn cycle_state(&self) -> bool {
        self.cycle_state
    }

    pub fn get_buf_ptr(&self) -> u64 {
        self.data.as_ptr() as u64
    }

    pub fn get_enque_ptr(&self) -> u64 {
        &self.data[self.enque] as *const UnknownTRB as u64
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }
}

pub struct EventRing {
    data: Vec<UnknownTRB>,
    cycle_state: bool,
    deque: usize,
}

impl EventRing {
    pub fn new(size: usize) -> Self {
        let data: Vec<UnknownTRB> = repeat_with(UnknownTRB::default).take(size).collect();

        Self {
            data,
            cycle_state: true,
            deque: 0,
        }
    }

    pub fn deque_index(&self) -> usize {
        self.deque
    }

    pub fn pop(&mut self) -> Option<UnknownTRB> {
        let trb = self.data[self.deque];

        if trb.cycle_bit() != self.cycle_state {
            return None;
        }

        self.deque += 1;
        if self.deque == self.data.len() {
            self.deque = 0;
            self.cycle_state = !self.cycle_state;
        }

        Some(trb)
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

fn dump_command_ring(ring: &ProducerRing) {
    for i in 0..ring.size() {
        if ring.data[i].cycle_bit() == ring.cycle_state() {
            let trb = unsafe { ring.data[i].into_cmd_trb() }
                .map_or("Invalid TRB".to_string(), |x| format!("{x:?}"));
            println!(
                "[{}{}{}]{}, {}",
                i,
                if ring.deque == i { " d" } else { "" },
                if ring.enque == i { " e" } else { "" },
                trb,
                ring.data[i].cycle_bit()
            );
        }
    }
}

pub fn dump_event_ring(ring: &EventRing) {
    for i in 0..ring.size() {
        if ring.data[i].cycle_bit() == ring.cycle_state() {
            let trb = unsafe { ring.data[i].into_event_trb() }
                .map_or("Invalid TRB".to_string(), |x| format!("{x:?}"));
            print!("{}", ring.data[i].cycle_bit() as usize);
            println!("[{}]{}, {}", i, trb, ring.data[i].cycle_bit());
        }
    }
    println!("\nd={}", ring.deque_index())
}

pub fn dump_trf_ring(ring: &ProducerRing) {
    for i in 0..ring.size() {
        if ring.data[i].cycle_bit() == ring.cycle_state() {
            let trb = unsafe { ring.data[i].into_trans_trb() }
                .map_or("Invalid TRB".to_string(), |x| format!("{x:?}"));
            println!(
                "[{}{}{}]{}, {}",
                i,
                if ring.deque == i { " d" } else { "" },
                if ring.enque == i { " e" } else { "" },
                trb,
                ring.data[i].cycle_bit()
            );
        }
    }
}
