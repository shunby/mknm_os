use core::{mem::{size_of, size_of_val}, slice::from_raw_parts};

use alloc::string::String;
use alloc::string::ToString;

use crate::{memory_manager::LazyInit, pci::io_in_32};

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

unsafe fn sum_bytes_unchecked<T>(obj: &T, until: usize) -> u8 {
    let bytes: &[u8] = from_raw_parts(obj as *const T as *const u8, until);
    bytes[..until].iter().fold(0, |a, b|{((a as u32 + *b as u32) & 0xff) as u8})
}

fn sum_bytes<T>(obj: &T, until: usize) -> u8 {
    assert!(size_of_val(obj) >= until);
    unsafe {
        sum_bytes_unchecked(obj, until)
    }
}

impl RSDP {
    fn is_valid(&self) -> bool {
        if String::from_utf8(self.signature.to_vec()).unwrap() != "RSD PTR " {
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

#[repr(C, packed)]
pub struct DescriptionHeader {
    signature: [u8;4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8;6],
    oem_table_id: [u8;8],
    oem_revision: u32,
    creater_id: u32,
    creater_revision: u32,
}

impl DescriptionHeader {
    fn is_valid(&self, expected_signature: &[u8]) -> bool {
        unsafe {
            self.signature == expected_signature 
            && sum_bytes_unchecked(self, self.length as usize) == 0
        }
    }
}

struct XSDT<'a>(&'a DescriptionHeader);

impl<'a> XSDT<'a> {
    const fn count(&self) -> usize {
        (self.0.length as usize - size_of::<DescriptionHeader>()) / size_of::<u64>()
    }
    
    unsafe fn entry(&self, index: usize) -> *const DescriptionHeader{
        assert!(index < self.count());
        let table_begin = self.0 as *const DescriptionHeader as u64 + size_of::<DescriptionHeader>() as u64;
        core::ptr::read_unaligned((table_begin + (index * size_of::<u64>()) as u64) as *const *const DescriptionHeader)
    }
}

#[repr(C, packed)]
struct FADT {
    header: DescriptionHeader,
    reserved1: [u8;76-size_of::<DescriptionHeader>()],
    pm_tmr_blk: u32,
    reserved2: [u8;112-80],
    flags: u32,
    reserved3: [u8; 276-116]
}

impl FADT {
    unsafe fn from_header(header: &DescriptionHeader) -> & FADT {
        &*(header as *const DescriptionHeader as *const FADT)
    }
}

static FADT: LazyInit<&FADT> = LazyInit::new();

pub unsafe fn initialize(rsdp: &RSDP) {
    if !rsdp.is_valid() {
        panic!("RSDP is not valid");
    }

    let xsdt_header = &*(rsdp.xsdt_address as *const DescriptionHeader);
    if !xsdt_header.is_valid("XSDT".as_bytes()) {
        panic!("XSDT is not valid");
    }

    let xsdt = XSDT(xsdt_header);

    let fadt = (0..xsdt.count()).find_map(|i: usize|{
        let entry = &*xsdt.entry(i);
        if entry.is_valid(b"FACP") {
            Some(entry)
        } else {
            None
        }
    }).expect("FADT is not found in XSDT");

    FADT.lock().init(FADT::from_header(fadt));
}
const PM_TIMER_FREQ: u32 = 3579545;
pub fn wait_millis(msec: u32) {
    let fadt = FADT.lock();
    let pm_timer_is_32 = (fadt.flags >> 8) & 1 != 0;

    unsafe {
        let start = io_in_32(fadt.pm_tmr_blk as u16);
        let mut end = start.wrapping_add(PM_TIMER_FREQ * msec / 1000);
        if !pm_timer_is_32 { // 24bit
            end &= 0x00ffffff;
        }
        
        if end < start { // overflow, wait until `fadt.pm_tmr_blk == 0`
            while io_in_32(fadt.pm_tmr_blk as u16) >= start {} 
        }
        while io_in_32(fadt.pm_tmr_blk as u16) < end {}
    }
}