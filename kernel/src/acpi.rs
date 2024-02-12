use core::{mem::{size_of, size_of_val, transmute}, slice::from_raw_parts};

use alloc::string::String;
use alloc::string::ToString;

#[repr(C, packed)]
pub struct RSDP {
    signature: [u8;8],
    checksum: u8,
    oem_id: [u8;6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt_address: u64,
    extendeed_checksum: u8,
    reserved: [u8; 3]
}

fn sum_bytes<T>(obj: &T, until: usize) -> u8 {
    assert!(size_of_val(obj) >= until);
    unsafe {
        let bytes: &[u8] = from_raw_parts(obj as *const T as *const u8, until);
        bytes[..until].iter().fold(0, |a, b|{((a as u32 + *b as u32) & 0xff) as u8})
    }
}

impl RSDP {
    fn is_valid(&self) -> bool {
        if String::from_utf8(self.signature.to_vec()).unwrap() != *"RSD PTR " {
            return false;
        }
        if self.revision != 2 {
            return false;
        }
        if sum_bytes(self, 20) != 0 {
            return false;
        }
        if sum_bytes(self, 36) != 0 {
            return  false;
        }

        true
    }
}

pub fn initialize(rsdp: &RSDP) {
    if !rsdp.is_valid() {
        panic!("RSDP is not valid");
    }

}