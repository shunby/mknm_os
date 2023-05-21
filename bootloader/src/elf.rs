use core::{mem::{size_of, transmute}, slice::from_raw_parts};

use alloc::vec::Vec;

type Elf64_Addr = u64;
type Elf64_Off = u64;
type Elf64_Half = u16;
type Elf64_Word = u32;
type Elf64_Sword = i32;
type Elf64_Xword = u64;
type Elf64_Sxword = i64;

const EI_NIDENT: usize = 16;

#[repr(C)]
pub struct Elf64_Ehdr {
    pub e_ident: [u8; EI_NIDENT],
    pub e_type: Elf64_Half,
    pub e_machine: Elf64_Half,
    pub e_version: Elf64_Word,
    pub e_entry: Elf64_Addr,
    pub e_phoff: Elf64_Off,
    pub e_shoff: Elf64_Off,
    pub e_flags: Elf64_Word,
    pub e_ehsize: Elf64_Half,
    pub e_phentsize: Elf64_Half,
    pub e_phnum: Elf64_Half,
    pub e_shentsize: Elf64_Half,
    pub e_shnum: Elf64_Half,
    pub e_shstrndx: Elf64_Half,
}

#[repr(C)]
pub struct Elf64_Phdr {
    pub p_type: Elf64_PhdrType,
    pub p_flags: Elf64_Word,
    pub p_offset: Elf64_Off,
    pub p_vaddr: Elf64_Addr,
    pub p_paddr: Elf64_Addr,
    pub p_filesz: Elf64_Xword,
    pub p_memsz: Elf64_Xword,
    pub p_align: Elf64_Xword,
}

#[repr(u16)]
#[derive(PartialEq)]
pub enum Elf64_PhdrType {
    PT_NULL     = 0,
    PT_LOAD     = 1,
    PT_DYNAMIC  = 2,
    PT_INTERP   = 3,
    PT_NOTE     = 4,
    PT_SHLIB    = 5,
    PT_PHDR     = 6,
    PT_TLS      = 7,
}




#[repr(C)]
union D_UN_Type{
    d_val: Elf64_Xword,
    d_ptr: Elf64_Addr
}

#[repr(C)]

pub struct Elf64_Dyn {
    d_tag: Elf64_Sxword,
    d_un: D_UN_Type,
}


pub fn read_elf<'a>(buffer: &'a [u8]) -> (&'a Elf64_Ehdr, &'a [Elf64_Phdr]) {
    let ehdr: &Elf64_Ehdr = unsafe {&*(buffer.as_ptr() as *const Elf64_Ehdr)};
    let phdrs: &[Elf64_Phdr] = 
        unsafe {
            &*(from_raw_parts(
                buffer.as_ptr().offset(ehdr.e_phoff as isize) as *const Elf64_Phdr,
                ehdr.e_phnum as usize)
            )
        };
    (ehdr, phdrs)
}

pub fn calc_load_address_range(phdrs: &[Elf64_Phdr]) -> (u64, u64) {
    let mut first = u64::MAX;
    let mut last = 0;
    for phdr in phdrs.iter().filter(|h|h.p_type == Elf64_PhdrType::PT_LOAD) {
        first = u64::min(first, phdr.p_vaddr);
        last = u64::max(last, phdr.p_vaddr + phdr.p_memsz);
    }
    return (first, last);
}
