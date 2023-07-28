#![allow(non_camel_case_types)]
#![allow(unused)]
#![warn(unused_imports, unused_import_braces)]
use core::slice::from_raw_parts;

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
    pub e_entry: Elf64_Addr, // エントリポイントのアドレス
    pub e_phoff: Elf64_Off, // プログラムヘッダのオフセット
    pub e_shoff: Elf64_Off,
    pub e_flags: Elf64_Word,
    pub e_ehsize: Elf64_Half,
    pub e_phentsize: Elf64_Half, // プログラムヘッダの各エントリのサイズ
    pub e_phnum: Elf64_Half,     // プログラムヘッダの数
    pub e_shentsize: Elf64_Half,
    pub e_shnum: Elf64_Half,
    pub e_shstrndx: Elf64_Half,
}

/**
 * プログラムヘッダの各要素
 * ELFファイル中の p_offset:p_offset+p_filesz の内容が、
 * メモリ上の p_vaddr:p_vaddr+p_memsz に配置される
 */
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

impl Elf64_Phdr {
    pub fn inmem_range(&self) -> (u64, u64) {
        (self.p_vaddr, self.p_vaddr + self.p_memsz)
    }

    pub fn inmem_size(&self) -> u64 {
        self.p_memsz
    }

    pub fn infile_range(&self) -> (u64, u64) {
        (self.p_offset, self.p_offset + self.p_filesz)
    }

    pub fn infile_size(&self) -> u64 {
        self.p_filesz
    }
}

#[repr(u16)]
#[derive(PartialEq)]
pub enum Elf64_PhdrType {
    PT_NULL = 0,
    PT_LOAD = 1,
    PT_DYNAMIC = 2,
    PT_INTERP = 3,
    PT_NOTE = 4,
    PT_SHLIB = 5,
    PT_PHDR = 6,
    PT_TLS = 7,
}

#[repr(C)]
union D_UN_Type {
    d_val: Elf64_Xword,
    d_ptr: Elf64_Addr,
}

#[repr(C)]

pub struct Elf64_Dyn {
    d_tag: Elf64_Sxword,
    d_un: D_UN_Type,
}

pub struct ElfFile<'a> {
    pub elf_header: &'a Elf64_Ehdr,
    pub prog_headers: &'a [Elf64_Phdr]
}

impl <'a> ElfFile<'a> {
    pub unsafe fn from_buffer(buffer: &[u8]) -> Self{
        let elf_header: &Elf64_Ehdr = &*(buffer.as_ptr() as *const Elf64_Ehdr);
        
        let prog_headers: &[Elf64_Phdr] = from_raw_parts(
            buffer.as_ptr().offset(elf_header.e_phoff as isize) as *const Elf64_Phdr,
            elf_header.e_phnum as usize,
        );

        Self { elf_header, prog_headers }
    }

    pub fn calc_load_address_range(&self) -> (u64, u64){
        let mut first = u64::MAX;
        let mut last = 0;
        for phdr in self.prog_headers.iter().filter(|h| h.p_type == Elf64_PhdrType::PT_LOAD) {
            let mem_range = phdr.inmem_range();
            first = u64::min(first, mem_range.0);
            last = u64::max(last, mem_range.1);
        }
        (first, last)
    }

}
