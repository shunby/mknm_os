use core::{arch::global_asm, mem::size_of_val};

use bitfield::bitfield;

use crate::interrupt::DescriptorType;

pub const KERNEL_CS: u16 = 1 << 3;
pub const KERNEL_SS: u16 = 2 << 3;

bitfield! {
    pub struct SegmentDescriptor(u64);
    u16;
    limit_low , set_limit_low: 15,0;
    base_low, set_base_low: 31,16;
    base_middle, set_base_middle: 39,32;
    type_, set_type_: 43,40;
    system_segment, set_system_segment: 44;
    descriptor_privilege_level, set_descriptor_privilege_level: 46,45;
    present, set_present: 47;
    limit_high, set_limit_high: 51,48;
    available, set_available: 52;
    long_mode, set_long_mode: 53;
    default_operation_size , set_default_operation_size: 54;
    granularity, set_granularity: 55;
    base_high, set_base_high: 63,56;
}

impl SegmentDescriptor {
    fn set_base(&mut self, base: u32) {
        self.set_base_low(base as u16);
        self.set_base_middle((base >> 16) as u16 & 0xff);
        self.set_base_high((base >> 24) as u16 & 0xff);
    }

    fn set_limit(&mut self, limit: u32) {
        self.set_limit_low(limit as u16 & 0xfff);
        self.set_limit_high((limit >> 16) as u16 & 0xf);
    }
}

fn set_code_segment(type_: DescriptorType, dpl: u16, base: u32, limit: u32) -> SegmentDescriptor {
    let mut desc = SegmentDescriptor(0);
    desc.set_base(base);
    desc.set_limit(limit);
    desc.set_type_(type_ as u16);
    desc.set_system_segment(true);
    desc.set_descriptor_privilege_level(dpl);
    desc.set_present(true);
    desc.set_available(false);
    desc.set_long_mode(true);
    desc.set_default_operation_size(false);
    desc.set_granularity(true);
    desc
}

fn set_data_segment(type_: DescriptorType, dpl: u16, base: u32, limit: u32) -> SegmentDescriptor {
    let mut desc = set_code_segment(type_, dpl, base, limit);
    desc.set_long_mode(false);
    desc.set_default_operation_size(true);
    desc
}

static mut GLOBAL_DESCRIPTOR_TABLE: [SegmentDescriptor; 3] = [SegmentDescriptor(0), SegmentDescriptor(0), SegmentDescriptor(0)];
pub fn setup_segments() {
    unsafe {
        GLOBAL_DESCRIPTOR_TABLE[0] = SegmentDescriptor(0);
        GLOBAL_DESCRIPTOR_TABLE[1] = set_code_segment(DescriptorType::ExecuteRead, 0, 0, 0xfffff);
        GLOBAL_DESCRIPTOR_TABLE[2] = set_data_segment(DescriptorType::LDTOrReadWrite, 0, 0, 0xfffff);
        load_gdt();
        set_ds_es_fs_gs(0);
        set_cs_ss(
            KERNEL_CS, // GLOBAL_DESCRIPTOR_TABLE[1]
            KERNEL_SS   // GLOBAL_DESCRIPTOR_TABLE[2]
        );

    }
}

fn load_gdt() {
    unsafe {_load_gdt(size_of_val(&GLOBAL_DESCRIPTOR_TABLE) as u16 - 1, &GLOBAL_DESCRIPTOR_TABLE as *const _ as u64);}
}

extern "sysv64" {
    fn _load_gdt(limit: u16, offset: u64);
    fn set_ds_es_fs_gs(value: u16);
    fn set_cs_ss(cs: u16, ss: u16);
}
global_asm!(r#"
_load_gdt:
    push rbp
    mov rbp, rsp
    sub rsp, 10
    mov [rsp], di
    mov [rsp + 2], rsi
    lgdt [rsp]
    mov rsp, rbp
    pop rbp
    ret
set_ds_es_fs_gs:
    mov ds, di
    mov es, di
    mov fs, di
    mov gs, di
    ret
set_cs_ss:
    push rbp
    mov rbp, rsp
    mov ss, si
    lea rax, .next
    push rdi
    push rax
    rex64 retf
.next:
    mov rsp, rbp
    pop rbp
    ret
"#);