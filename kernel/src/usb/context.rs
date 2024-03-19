use core::fmt;
use core::{alloc::Layout, slice};

use ::xhci::context;

use super::xhci::{AlignedAlloc, LinearMapper};
use super::util;
use alloc::{boxed::Box, collections::BTreeMap, alloc::alloc, vec::Vec};

/// Device Context Manager
pub struct DCManager {
    /// Device Context Base Address Array
    dcbaa: Box<[u64]>,
    /// maps slot id to device context
    contexts: BTreeMap<usize, DeviceContext>,
    ctx_size: ContextSize,
    scratchpad_buf_arr: Option<Box<[u64], AlignedAlloc<64>>>
}


pub enum DeviceContext {
    DC32Byte(Box<context::Device32Byte, AlignedAlloc<64>>),
    DC64Byte(Box<context::Device64Byte, AlignedAlloc<64>>),
}

#[derive(Debug, Clone, Copy)]
pub enum ContextSize {
    Csz32Bytes,
    Csz64Bytes,
}

pub fn init_dcbaa(regs: &mut ::xhci::registers::Registers<LinearMapper>) -> DCManager {
    let max_slots = regs
        .capability
        .hcsparams1
        .read_volatile()
        .number_of_device_slots() as usize;
    let num_ports = regs.capability.hcsparams1.read_volatile().number_of_ports();
    let ctx_size = 
        if regs.capability.hccparams1.read_volatile().context_size() {
            ContextSize::Csz64Bytes
        } else {
            ContextSize::Csz32Bytes
        };
    println!("max_slots={max_slots}");
    println!("num_ports={num_ports}");
    println!("ctx_size={ctx_size:?}");

    regs.operational.config.update_volatile(|cfg| {
        cfg.set_max_device_slots_enabled(max_slots as u8);
    });
    let num_scratch_pads = regs
        .capability
        .hcsparams2
        .read_volatile()
        .max_scratchpad_buffers() as usize;
    let pagesize_bit = util::find_lsb(regs.operational.pagesize.read_volatile().get());
    let page_size = 1 << (12 + pagesize_bit);
    println!("scratch_pads={num_scratch_pads}");
    println!("page_size={page_size}B");
    let dcm = unsafe {DCManager::new(max_slots, ctx_size, num_scratch_pads, page_size)};
    
    regs.operational.dcbaap
        .update_volatile(|x| x.set(dcm.dcbaa.as_ptr() as u64));
    dcm
}

impl DCManager {
    unsafe fn new(max_slots: usize, ctx_size: ContextSize, num_scratch_pads: usize, page_size: usize) -> Self{
        let mut dcbaa: Box<[u64]> = util::aligned_zeros(max_slots, 64);
        let scratchpad_buf_arr = if num_scratch_pads > 0 {
            Some(make_scratchpad_buf_arr(num_scratch_pads, page_size))
        } else {
            None
        };

        Self {
            dcbaa,
            scratchpad_buf_arr,
            ctx_size,
            contexts: BTreeMap::new()
        }
    }

    pub fn context_at(&self, i: usize) -> &DeviceContext {
        assert!(1 <= i && i <= self.num_slots());
        &self.contexts[&i]
    }

    pub fn num_slots(&self) -> usize {
        self.dcbaa.len() - 1
    }

    pub fn init_at(&mut self, i: usize) {
        assert!(1 <= i && i <= self.num_slots());
        self.contexts[&i] = DeviceContext::new(self.ctx_size);
    }

    pub fn context_size(&self) -> ContextSize {
        self.ctx_size
    }
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
    pub fn handler(&self) -> &dyn context::DeviceHandler {
        match self {
            DeviceContext::DC32Byte(dev) => dev.as_ref(),
            DeviceContext::DC64Byte(dev) => dev.as_ref(),
        }
    }

    pub fn handler_mut(&mut self) -> &mut dyn context::DeviceHandler {
        match self {
            DeviceContext::DC32Byte(dev) => dev.as_mut(),
            DeviceContext::DC64Byte(dev) => dev.as_mut(),
        }
    }

    pub fn get_address(&self) -> u64 {
        match self {
            DeviceContext::DC32Byte(dev) => dev.as_ref() as *const context::Device32Byte as u64,
            DeviceContext::DC64Byte(dev) => dev.as_ref() as *const context::Device64Byte as u64,
        }
    }

    pub fn get_size(&self) -> ContextSize {
        match *self {
            DeviceContext::DC32Byte(_) => ContextSize::Csz32Bytes,
            DeviceContext::DC64Byte(_) => ContextSize::Csz64Bytes,
        }
    }
}

// impl fmt::Debug for DeviceContext {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         dump_slot_ctx(f, self.handler().slot())?;
//         for i in 1..=31 {
//             let ep = self.handler().endpoint(i);
//             if ep.endpoint_state() != EndpointState::Disabled {
//                 f.write_fmt(format_args!("[{i}] "))?;
//                 dump_ep_ctx(f, ep)?;
//             }
//         }
//         Ok(())
//     }
// }

impl From<ContextSize> for usize {
    fn from(value: ContextSize) -> Self {
        match value {
            ContextSize::Csz32Bytes => 32,
            ContextSize::Csz64Bytes => 64,
        }
    }
}

unsafe fn make_scratchpad_buf_arr(num_scratch_pads: usize, page_size: usize) -> Box<[u64], AlignedAlloc<64>> {
    let mut page_ptrs: Vec<u64, AlignedAlloc<64>> =
            Vec::with_capacity_in(num_scratch_pads, AlignedAlloc::<64> {});
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
    let scratchpad_buf_arr = page_ptrs.into_boxed_slice();
    scratchpad_buf_arr
}
