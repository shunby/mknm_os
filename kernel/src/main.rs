#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

mod frame_buffer;
#[macro_use]
mod font;
mod graphics;
mod console;
mod pci;
mod mouse;
mod interrupt;
mod memory_map;
mod segment;

use core::mem::{MaybeUninit, transmute};
use core::panic::PanicInfo;
use core::arch::{asm, global_asm};
use core::ptr::write_volatile;

use console::Console;
use frame_buffer::{FrameBufferRaw};
use graphics::{Vec2};
use interrupt::{set_idt_entry, IVIndex, InterruptDescriptor, InterruptDescriptorAttribute, DescriptorType, load_idt};
use memory_map::{MemoryMapRaw, MemoryMap};
use mouse::MouseCursor;
use pci::{PCIController, PCIDevice, configure_msi_fixed_destination};

use usb_bindings::raw::{usb_xhci_ConfigurePort, usb_xhci_ProcessEvent, usb_set_default_mouse_observer, usb_xhci_Controller};

use crate::graphics::Graphics;
use crate::interrupt::set_interrupt_flag;
use crate::memory_map::MemoryDescriptor;
use crate::segment::setup_segments;


const LOGO: [u64;26] = [
    0b00000000000111111111111111100000000,
    0b00001111111000100000000000011111000,
    0b00111100000000000000000000001111000,
    0b01100000000000000000000000011011000,
    0b11000000000000001000000000010001110,
    0b11000100000010001000010000110001111,
    0b11101100000010001000010000001111000,
    0b00111000000010001100010100000011000,
    0b00011110000010001111110100000010000,
    0b00011011111111101111011100000010000,
    0b00101010000000011111111100000010000,
    0b00101010000000000000000100000010000,
    0b00101010111100001111000100000010000,
    0b00101010000000000000000100000110000,
    0b00100110000000000000000100000100000,
    0b00100010000011000000000100000100000,
    0b00100011000110110000000101000100000,
    0b00100011110000000000001111000100000,
    0b00100110001111111111110100000100000,
    0b00100100000000000000000100000100000,
    0b00100100000000000000000010001000000,
    0b00111100000000000000000100001000000,
    0b01111000000000000000000100001000000,
    0b01100000000000000000000100001000000,
    0b00000000000000000000001100010000000,
    0b00000000000000000000001111110000000,
];

// FIXME: not thread-safe or interruption-safe
static mut CONSOLE: MaybeUninit<Console> = MaybeUninit::uninit();
static mut MOUSE: MaybeUninit<MouseCursor> = MaybeUninit::uninit();
static mut XHC: MaybeUninit<usb_xhci_Controller> = MaybeUninit::uninit();
static mut GRAPHICS: MaybeUninit<Graphics> = MaybeUninit::uninit();

fn get_console() -> &'static mut Console<'static> {
    unsafe { CONSOLE.assume_init_mut() }
}

fn get_mouse() -> &'static mut MouseCursor<'static> {
    unsafe { MOUSE.assume_init_mut() }
}

fn get_xhc() -> &'static mut usb_xhci_Controller {
    unsafe { XHC.assume_init_mut() }
}

fn get_graphics() -> &'static mut Graphics<'static> {
    unsafe { GRAPHICS.assume_init_mut() }
}

fn scan_pci_devices() {
    let mut pci = PCIController::new();
    unsafe {
        pci.scan_all_bus().unwrap();
        for dev in pci.get_devices() {
            let classcode = dev.read_class_code();

            let index = dev.get_index();
            print!(
                index.0, ".", index.1, ".", index.2, 
                ": head ", dev.read_header_type(),
                ", vend ", dev.read_vendor_id(),
                ", class ", classcode.base, classcode.sub, classcode.interface , "\n"
            );        
        }
    }

}

fn find_xhc_device() -> PCIDevice {
    let mut pci = PCIController::new();
    unsafe {
        pci.scan_all_bus().unwrap();

        // look for xhc devices, prioritizing Intel ones
        let mut xhc_device = None;
        for dev in pci.get_devices() {
            if dev.read_class_code().matches(0x0c, 0x03, 0x30) {
                xhc_device = Some(dev);
                if dev.read_vendor_id() == 0x8086 {
                    break;
                }
            }
        }
        xhc_device.unwrap().clone()
    }
}

unsafe extern "C" fn mouse_observer(x: i8, y: i8) {
    get_mouse().move_relative(Vec2::new(x as i32, y as i32));
}

fn initialize_xhci_controller(xhc: &PCIDevice) -> usb_xhci_Controller {
    unsafe {
        let xhc_bar = xhc.read_bar(0);
        let xhc_mmio_base = xhc_bar & !(0b1111 as u64);
        let mut xhc = usb_bindings::raw::usb_xhci_Controller::new(xhc_mmio_base as usize);
        let err = xhc.Initialize();
        print!("xhc_mmio_base: ", xhc_mmio_base, "\n");
        print!("xhc_bar: ", xhc_bar, "\n");
        print!("initialize xhc: ", err.code_, "\n");
        xhc.Run();
        print!("starting xhc\n");

        usb_set_default_mouse_observer(Some(mouse_observer));
        for i in 1..=xhc.max_ports_ {
            let mut port = xhc.PortAt(i);
            if port.IsConnected() {
                let err = usb_xhci_ConfigurePort(&mut xhc, &mut port);
                if err.code_ != 0 {
                    print!("failed to configure port: ", err.code_, "\n");
                }
            }
        }
        xhc
    }
}

unsafe extern "C" fn print_c(mut s: *const cty::c_char) {
    let mut buf = [0u8;128];
    let mut seek = 0;
    while *s != 0 && seek < buf.len(){
        buf[seek] = *s as u8;
        s = s.offset(1);
        seek += 1;
    }
    print!(&buf[..seek]);
}

fn print_memmap(memmap: &MemoryMap) {
    for entry in memmap.entries() {
        print!(
            "type: ", entry.type_.to_str(),
            ", phys: ", entry.physical_start, " - ", (entry.physical_start as u128 + entry.num_pages as u128 * 4096 - 1),
            ", pages: ", entry.num_pages,
            ", attr: ", entry.attribute,
            "\n"
        );
        
    }
}

#[repr(align(16))]
struct Stack ([u8;1024*1024]);

#[no_mangle]
static mut kernel_main_stack: Stack = Stack([0u8;1024*1024]);

#[no_mangle]
#[allow(unreachable_code)]
pub extern "sysv64" fn KernelMain(fb: *const FrameBufferRaw, mm: *const MemoryMapRaw) -> ! {
    unsafe { 
        asm!("lea rsp, [kernel_main_stack + 1024 * 1024]");
        KernelMain2(fb, mm);
        asm!(
            "   hlt",
            "   jmp .fin"
        );
    }
}

#[no_mangle]
pub extern "sysv64" fn KernelMain2(fb: *const FrameBufferRaw, mm: *const MemoryMapRaw) -> ! {
    unsafe {
        GRAPHICS = transmute(MaybeUninit::new(Graphics::new((&*fb).into())));
        CONSOLE = transmute(MaybeUninit::new(Console::new(
            get_graphics(),
            (255,255,255),
            (100,100,100)
        )));
        MOUSE = transmute(MaybeUninit::new(MouseCursor::new(
            get_graphics(),
            (100,100,100),
            Vec2::new(200,300)
        )));
    }
    unsafe{
        usb_bindings::raw::SetLogLevel(1);
        usb_bindings::raw::SetPrintFn(Some(print_c));
    }
    
    unsafe {
        let graphics = get_graphics();
        graphics.fill_rect((0,0).into(), graphics.resolution().into(), (100,100,100));
        graphics.draw_bitpattern((100u32,100u32).into(), &LOGO, (0,0,255), 5);

        let memmap: MemoryMap = (&*mm).into();
        // print_memmap(&memmap);
        scan_pci_devices();
        setup_segments();
        set_idt_entry(
            IVIndex::XHCI, 
            InterruptDescriptor::new(
                get_cs(), 
                InterruptDescriptorAttribute::new(0, DescriptorType::InterruptGate), 
                transmute(interrupt_handler as *const fn())
            )
        );
        load_idt();
        let xhc = find_xhc_device();
        let local_apic_id = *(0xfee00020 as *const u32) >> 24;
        print!("apic_id: ", local_apic_id, "\n");
        configure_msi_fixed_destination(&xhc, local_apic_id as u8, IVIndex::XHCI as u8);
    
        XHC = MaybeUninit::new(initialize_xhci_controller(&xhc));
        set_interrupt_flag(true);
    }

    print!("finish\n");
    unsafe {
        loop {
            asm!("hlt");
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    if let Some(loc) = _info.location() {
        print!("panicked: ", loc.file().as_bytes(), ": ", loc.line());
    }
    loop {}
}

extern "sysv64" {
    fn get_cs() -> u16;
}
global_asm!(r#"
get_cs:
    xor eax, eax
    mov ax, cs
    ret
"#);

#[allow(dead_code)]
extern "x86-interrupt" fn interrupt_handler() {
    // print!("mouse move!\n");
    unsafe {
        let xhc = get_xhc();
        while (&*xhc.PrimaryEventRing()).HasFront()  {
            let err = usb_xhci_ProcessEvent(xhc);
            if err.code_ != 0 {
                print!("error while processevent: ", err.code_, "\n");
            }
        }
    }
    notify_end_of_interrupt();
}

fn notify_end_of_interrupt() {
    unsafe {
        let end_of_interrupt: *mut u32 = transmute(0xfee000b0u64);
        write_volatile(end_of_interrupt, 0);
    }

}