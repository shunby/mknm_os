use core::{alloc::Layout, mem::{self, size_of, MaybeUninit}, slice::{self, from_raw_parts_mut}};
use ::alloc::{boxed::Box, alloc::alloc};

pub unsafe fn aligned_zeros(len: usize, align: usize) -> Box<[u64]> {
    let data = slice::from_raw_parts_mut(
        alloc(Layout::from_size_align(len * 8, align).unwrap()) as *mut u64,
        len,
    );
    data.fill(0);
    Box::from_raw(data)
}


pub fn find_lsb(bits: u16) -> usize {
    for i in 0..15 {
        if (bits >> i) & 1 == 1 {
            return i;
        }
    }
    16
}