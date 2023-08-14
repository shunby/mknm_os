#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]

mod frame_buffer;
#[macro_use]
mod font;
mod graphics;
mod console;
mod pci;
mod mouse;
mod interrupt;
mod memory_map;
mod memory_manager;
mod segment;
mod paging;
mod window;
mod xhci;
mod timer;

#[macro_use]
extern crate alloc;

use core::alloc::Layout;
use core::mem::transmute;
use core::panic::PanicInfo;
use core::arch::{asm, global_asm};
use core::ptr::write_volatile;
use core::str::from_utf8;

use alloc::boxed::Box;
use console::Console;
use frame_buffer::FrameBufferRaw;
use graphics::{Vec2, PixelWriter};
use interrupt::{set_idt_entry, IVIndex, InterruptDescriptor, InterruptDescriptorAttribute, DescriptorType, load_idt};
use memory_manager::LazyInit;
use memory_map::{MemoryMapRaw, MemoryMap};
use pci::{PCIController, PCIDevice, configure_msi_fixed_destination};

use usb_bindings::raw::{usb_xhci_ConfigurePort, usb_xhci_ProcessEvent, usb_xhci_Controller};
use window::LayeredWindowManager;

use crate::frame_buffer::FrameBuffer;
use crate::graphics::Graphics;
use crate::interrupt::set_interrupt_flag;
use crate::memory_manager::init_allocators;
use crate::mouse::draw_cursor;
use crate::paging::setup_identity_page_table;
use crate::segment::setup_segments;
use crate::timer::{start_lapic_timer, lapic_timer_elapsed, stop_lapic_timer, initialize_lapic_timer};
use crate::window::Window;
use crate::xhci::set_default_mouse_observer;


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

static CONSOLE: LazyInit<Console> = LazyInit::new();
static XHC: LazyInit<usb_xhci_Controller> = LazyInit::new();
static LAYERS: LazyInit<LayeredWindowManager> = LazyInit::new();

fn scan_pci_devices() {
    let mut pci = PCIController::new();
    unsafe {
        pci.scan_all_bus().unwrap();
        for dev in pci.get_devices() {
            let classcode = dev.read_class_code();

            let index = dev.get_index();
            println!(
                "{}.{}.{}: head {}, vend {}, class {} {} {}",
                    index.0, index.1, index.2,
                    dev.read_header_type(),
                    dev.read_vendor_id(),
                    classcode.base, classcode.sub, classcode.interface
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

fn initialize_xhci_controller(xhc: &PCIDevice, mouse_observer: impl Fn(i8, i8) + 'static) -> usb_xhci_Controller {
    unsafe {
        let xhc_bar = xhc.read_bar(0);
        let xhc_mmio_base = xhc_bar & !(0b1111 as u64);
        let mut xhc = usb_bindings::raw::usb_xhci_Controller::new(xhc_mmio_base as usize);
        let err = xhc.Initialize();
        println!("xhc_mmio_base: {}", xhc_mmio_base);
        println!("xhc_bar: {}", xhc_bar);
        println!("initialize xhc: {}", err.code_);
        xhc.Run();
        print!("starting xhc\n");

        set_default_mouse_observer(mouse_observer);
        for i in 1..=xhc.max_ports_ {
            let mut port = xhc.PortAt(i);
            if port.IsConnected() {
                let err = usb_xhci_ConfigurePort(&mut xhc, &mut port);
                if err.code_ != 0 {
                    println!("failed to configure port: {}", err.code_);
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
    print!("{}", from_utf8(&buf[..seek]).unwrap());
}

fn print_memmap(memmap: &MemoryMap) {
    for entry in memmap.entries() {
        println!(
            "type: {}, phys: {} - {}, pages: {}, attr: {}",
            entry.type_.to_str(),
            entry.physical_start, (entry.physical_start as u128 + entry.num_pages as u128 * 4096 - 1),
            entry.num_pages, 
            entry.attribute,
        );
        
    }
}

#[repr(align(16))]
struct Stack ([u8;1024*1024]);

#[no_mangle]
static mut kernel_main_stack: Stack = Stack([0u8;1024*1024]);

#[no_mangle]
#[allow(unreachable_code)]
pub unsafe extern "sysv64" fn KernelMain(fb: *const FrameBufferRaw, mm: *const MemoryMapRaw) -> ! {
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
pub unsafe extern "sysv64" fn KernelMain2(fb: *const FrameBufferRaw, mm: *const MemoryMapRaw) -> ! {
    let memmap: MemoryMap = (&*mm).into();
    setup_segments();
    setup_identity_page_table();
    init_allocators(&memmap);
    initialize_lapic_timer();

    let fb = FrameBuffer::new(fb);
    LAYERS.lock().init(LayeredWindowManager::new(Box::new(Graphics::new(fb))));
    let (mouse_window_id, console_window_id) = {
        let mut layer_mgr = LAYERS.lock();
    
        let mut mouse_window = Window::new(15, 24);
        mouse_window.set_transparent_color(Some((1,1,1)));
        draw_cursor(&mut mouse_window);
        let mouse_window_id = layer_mgr.new_layer(mouse_window);
    
        let console_window = Window::new(640, 480);
        let console_window_id = layer_mgr.new_layer(console_window);
        layer_mgr.up_down(console_window_id, 0);
        layer_mgr.up_down(mouse_window_id, 1);
        (mouse_window_id, console_window_id)
    };
    CONSOLE.lock().init(Console::new(  
        console_window_id,
        (255,255,255),
        (100,100,100)
    ));
    
    usb_bindings::raw::SetLogLevel(1);
    usb_bindings::raw::SetPrintFn(Some(print_c));

    // print_memmap(&memmap);
    scan_pci_devices();
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
    println!("apic_id: {}", local_apic_id);
    configure_msi_fixed_destination(&xhc, local_apic_id as u8, IVIndex::XHCI as u8);

    XHC.lock().init(initialize_xhci_controller(&xhc, move |dx,dy| {
        start_lapic_timer();
        {
            let mut layers = LAYERS.lock();
            layers.move_relative(mouse_window_id, (dx as i32, dy as i32).into());
            layers.draw();
        }
        println!("MouseObserver: elapsed = {}", lapic_timer_elapsed());
        stop_lapic_timer();
    }));
    set_interrupt_flag(true);
    
    LAYERS.lock().draw();
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
        print!("panicked: {}: {}", loc.file(), loc.line());
    }
    unsafe {
        loop {
            asm!("hlt");
        }
    }
}

#[alloc_error_handler]
fn oom(_: Layout) -> ! {
    print!("out of memory.");
    unsafe {
        loop {
            asm!("hlt");
        }
    }
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
        let mut xhc = XHC.lock();
        while (*xhc.PrimaryEventRing()).HasFront()  {
            let err = usb_xhci_ProcessEvent(xhc.get_mut());
            if err.code_ != 0 {
                println!("error while processevent: {}", err.code_);
            }
        }
    }
    notify_end_of_interrupt();
}

fn notify_end_of_interrupt() {
    unsafe {
        let end_of_interrupt = 0xfee000b0u64 as *mut u32;
        write_volatile(end_of_interrupt, 0);
    }
}
