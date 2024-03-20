use core::alloc::Layout;
use core::{fmt, slice};

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::alloc::alloc;
use xhci::context::{EndpointHandler, EndpointState, Input, Input32Byte, Input64Byte, InputHandler, SlotHandler};
use xhci::{context::{Device32Byte, Device64Byte, DeviceHandler}, Registers};

use crate::usb::util;

use super::xhci::{AlignedAlloc, LinearMapper};

pub struct Dcbaa {
    dcbaa: Box<[u64]>,
    contexts: BTreeMap<usize, DeviceContext>,
    ctx_size: ContextSize,
    scratchpad_buf_arr: Option<Box<[u64], AlignedAlloc<64>>>
}


#[derive(Debug, Clone, Copy)]
pub enum ContextSize {
    Csz32Bytes,
    Csz64Bytes,
}

pub enum DeviceContext {
    DC32Byte(Box<Device32Byte, AlignedAlloc<64>>),
    DC64Byte(Box<Device64Byte, AlignedAlloc<64>>),
}

pub fn init_dcbaa(regs: &mut Registers<LinearMapper>) -> Dcbaa{
    let max_slots = regs
        .capability
        .hcsparams1
        .read_volatile()
        .number_of_device_slots();
    let ctx_size = if regs.capability.hccparams1.read_volatile().context_size() {
        ContextSize::Csz64Bytes
    } else {
        ContextSize::Csz32Bytes
    };

    let num_scratch_pads = regs
        .capability
        .hcsparams2
        .read_volatile()
        .max_scratchpad_buffers() as usize;
    let pagesize_bit = util::find_lsb(regs.operational.pagesize.read_volatile().get());
    let page_size = 1 << (12 + pagesize_bit);

    let mut dcbaa: Box<[u64]> = unsafe {
        util::aligned_zeros(max_slots as usize + 1, 64)
    };

    let scratchpad_buf_arr = 
        if num_scratch_pads > 0 {
            let arr = make_scratchpad(num_scratch_pads, page_size);
            dcbaa[0] = arr.as_ref() as *const [u64] as *const u64 as u64;
            Some(arr)
        } else {
            None
        };
        
    regs.operational.config.update_volatile(|cfg| {
        cfg.set_max_device_slots_enabled(max_slots);
    });
    regs.operational.dcbaap.update_volatile(|x| x.set(dcbaa.as_mut_ptr() as u64));
    Dcbaa {
        dcbaa,
        contexts: BTreeMap::new(),
        ctx_size,
        scratchpad_buf_arr
    }
}

fn make_scratchpad(num_scratch_pads: usize, page_size: usize) -> Box<[u64], AlignedAlloc<64>> {
    let mut page_ptrs: Vec<u64, AlignedAlloc<64>> = Vec::with_capacity_in(num_scratch_pads, AlignedAlloc::<64> {});
    for _ in 0..num_scratch_pads {
        unsafe {
            let page = alloc(Layout::from_size_align(page_size, page_size).unwrap());
            if page.is_null() {
                panic!("Failed to allocate xHCI scratchpad buffer");
            }
            slice::from_raw_parts_mut(page, page_size).fill(0);
            page_ptrs.push(page as u64);
        }
    }

    page_ptrs.into_boxed_slice()
}

impl Dcbaa {
    pub fn get_context_at(&self, slot_id: usize) -> &DeviceContext {
        &self.contexts[&slot_id]
    }

    pub fn init_context_at(&mut self, slot_id: usize) {
        self.contexts.insert(slot_id, DeviceContext::new(self.ctx_size));
        self.dcbaa[slot_id] = self.contexts[&slot_id].get_address();
    }

    pub fn ctx_size(&self) -> ContextSize {
        self.ctx_size
    }
}


impl DeviceContext {
    pub fn new(size: ContextSize) -> Self {
        match size {
            ContextSize::Csz32Bytes => {
                Self::DC32Byte(Box::new_in(xhci::context::Device::new_32byte(), AlignedAlloc {}))
            }
            ContextSize::Csz64Bytes => {
                Self::DC64Byte(Box::new_in(xhci::context::Device::new_64byte(), AlignedAlloc {}))
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

impl fmt::Debug for DeviceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

fn dump_ep_ctx(f: &mut fmt::Formatter<'_>, ep: &dyn EndpointHandler) -> fmt::Result {
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

fn dump_slot_ctx(f: &mut fmt::Formatter<'_>, slot: &dyn SlotHandler) -> fmt::Result {
    f.write_fmt(format_args!(
        "[slot] speed: {} entries: {} port: {} state: {:?}\n",
        slot.speed(),
        slot.context_entries(),
        slot.root_hub_port_number(),
        slot.slot_state()
    ))
}
impl From<ContextSize> for usize {
    fn from(value: ContextSize) -> Self {
        match value {
            ContextSize::Csz32Bytes => 32,
            ContextSize::Csz64Bytes => 64,
        }
    }
}



impl fmt::Debug for InputContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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