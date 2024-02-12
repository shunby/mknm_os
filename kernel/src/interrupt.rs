use core::{mem::{MaybeUninit, size_of, transmute_copy}, arch::{global_asm, asm}, fmt::{Write, Formatter, Debug, Result}};

use bitfield::bitfield;
use cty::c_void;

// Interrupt Vector Index
#[derive(Debug, Clone, Copy)]
pub enum IVIndex {
    XHCI = 0x40,
    LapicTimer = 0x41
}

bitfield! {
    #[repr(C)]
    pub struct InterruptDescriptorAttribute(u16);
    u8;
    interrupt_stack_table, set_interrupt_stack_table: 2,0;
    type_, set_type: 11,8;
    descriptor_priv_level, set_descriptor_priv_level: 14,13;
    present, set_present: 15;
}

impl Debug for InterruptDescriptorAttribute {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.write_fmt(format_args!("{:x}", self.0))
    }
}

impl InterruptDescriptorAttribute {
    pub fn new(dpl: u8, type_: DescriptorType) -> Self {
        let mut res = InterruptDescriptorAttribute(0);
        res.set_present(true);
        res.set_descriptor_priv_level(dpl);
        res.set_type(type_ as u8);
        res.set_interrupt_stack_table(0);
        res
    }
}

#[repr(u8)]
pub enum DescriptorType {
    // system segment & gate descriptor types
    Upper8Bytes    = 0,
    LDTOrReadWrite = 2,
    TSSAvailable   = 9,
    TSSBusy        = 11,
    CallGate       = 12,
    InterruptGate  = 14,
    TrapGate       = 15,
    
    // code & data segment types
    // ReadWrite     = 2,
    ExecuteRead    = 10,
}

#[derive(Debug)]
#[repr(C)]
pub struct InterruptDescriptor {
    pub offset_low: u16,
    pub segment_selector: u16,
    pub attr: InterruptDescriptorAttribute,
    pub offset_middle: u16,
    pub offset_high : u32,
    pub reserved: u32,
}

impl InterruptDescriptor {
    pub fn new(segment_selector: u16, attr: InterruptDescriptorAttribute, offset: *const c_void) -> Self {
        Self {
            segment_selector,
            attr,
            reserved: 0,
            offset_low: (offset as u64 & 0xffff) as u16,
            offset_middle: ((offset as u64 >> 16) & 0xffff) as u16,
            offset_high: (offset as u64 >> 32) as u32
        }
    }
}

static mut IDT: [MaybeUninit<InterruptDescriptor>; 256] = unsafe{MaybeUninit::uninit().assume_init()};

pub fn set_idt_entry(index: IVIndex, entry: InterruptDescriptor) {
    unsafe {
        println!("IDT entry at {}", &IDT[index as usize] as *const _ as u64);
        println!("entry: {:?}", &entry);
        IDT[index as usize] = MaybeUninit::new(entry);
    }
}

pub fn load_idt() {
    unsafe {
        let limit = (size_of::<[MaybeUninit<InterruptDescriptor>; 256]>()-1) as u16;
        let offset = &IDT as *const _;
        println!("load_idt: limit={}, offser={}", limit, offset as u64);
        _load_idt(limit, offset);
    }
}

pub fn set_interrupt_flag(flag: bool) {
    unsafe {
        if flag {
            asm!("sti");
        } else {
            asm!("cli");
        }
    }
}

extern "sysv64" {
    fn _load_idt(limit: u16, offset: *const MaybeUninit<InterruptDescriptor>);
} 

global_asm!(r#"
_load_idt:
    push rbp
    mov rbp, rsp
    sub rsp, 10
    mov [rsp], di  
    mov [rsp + 2], rsi 
    lidt [rsp]
    mov rsp, rbp
    pop rbp
    ret
"#);