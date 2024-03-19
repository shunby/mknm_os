/// general assembly functions
use core::arch::global_asm;

extern "sysv64" {
    /// Read from IO address space
    pub fn io_in_32(addr: u16) -> u32;
    /// Write to IO address space
    pub fn io_out_32(addr: u16, data: u32);
    pub fn get_cr3() -> u64;
}

global_asm!(r#" 
.globl io_out_32
io_out_32:
    mov dx, di
    mov eax, esi
    out dx, eax
    ret
.globl io_in_32
io_in_32:
    mov dx, di
    in eax, dx
    ret
.globl get_cr3
get_cr3:
    mov rax, cr3
    ret
"#
);
