#![cfg_attr(not(test), no_std)]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(allocator_api)]

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
mod timer;
mod usb;

#[macro_use]
extern crate alloc;

use core::alloc::Layout;
use core::mem::transmute;
use core::panic::PanicInfo;
use core::arch::{asm, global_asm};
use core::ptr::write_volatile;
use core::str::from_utf8;

use console::Console;
use frame_buffer::FrameBufferRaw;
use graphics::PixelWriter;
use interrupt::{set_idt_entry, IVIndex, InterruptDescriptor, InterruptDescriptorAttribute, DescriptorType, load_idt};
use memory_manager::LazyInit;
use memory_map::{MemoryMapRaw, MemoryMap};
use pci::{PCIController, PCIDevice, configure_msi_fixed_destination};

use window::LayeredWindowManager;

use crate::frame_buffer::{FrameBuffer, set_default_pixel_format};
use crate::interrupt::set_interrupt_flag;
use crate::memory_manager::init_allocators;
use crate::mouse::draw_cursor;
use crate::paging::setup_identity_page_table;
use crate::segment::setup_segments;
use crate::timer::{start_lapic_timer, lapic_timer_elapsed, stop_lapic_timer, initialize_lapic_timer};
use crate::usb::xhci::initialize_xhci;
use crate::window::Window;


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
static LAYERS: LazyInit<LayeredWindowManager> = LazyInit::new();

fn scan_pci_devices() -> PCIController {
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
    pci
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
    set_interrupt_flag(false);   

    let mut fb = FrameBuffer::from_raw(fb);
    let (display_width, display_height) = fb.resolution();
    let display_pitch = fb.pixels_per_scanline();
    set_default_pixel_format(fb.pixel_format());
    for y in 0..display_height {
        for x in 0..display_width {    
            fb.write((x as i32, y as i32).into(), (127,127,127));
        }
    }
    LAYERS.lock().init(LayeredWindowManager::new(fb));
    let (mouse_window_id, console_window_id) = {
        let mut layer_mgr = LAYERS.lock();
    
        let mut mouse_window = Window::new(15, 24);
        mouse_window.set_transparent_color(Some((1,1,1)));
        draw_cursor(&mut mouse_window);
        let mouse_window_id = layer_mgr.new_layer(mouse_window);
    
        let console_window = Window::new(display_width as usize, display_height as usize);
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
    
    println!("resolution: {}x{}, pitch={}", display_width, display_height, display_pitch);

    // print_memmap(&memmap);
    let pci = scan_pci_devices();
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

    let intel_ehci_found = pci.get_devices().iter().any(|dev|{
        dev.read_vendor_id() == 0x8086 &&  dev.read_class_code().matches(0x0c, 0x03, 0x20) 
    });

    initialize_xhci(xhc, intel_ehci_found, move |report| {
        {
            let (dx,dy) = (report.dx(), report.dy());
            let mut layers = LAYERS.lock();
            let window = layers.get_layer_mut(mouse_window_id);
            let new_pos = (window.pos() + (dx as i32, dy as i32).into()).clamp((0,0).into(), (display_width as i32, display_height as i32).into());
            window.move_to(new_pos);
            layers.draw();
        }
    });

    print!("finish\n");
    // LAYERS.lock().draw();
    set_interrupt_flag(true);   
    unsafe {
        loop {
            asm!("hlt");
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    println!("{_info}");
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
    // print!("interrupt!\n");
    
    usb::xhci::run_xhci_tasks();
    
    // println!("interrupt end.");
    notify_end_of_interrupt();
}

fn notify_end_of_interrupt() {
    unsafe {
        let end_of_interrupt = 0xfee000b0u64 as *mut u32;
        write_volatile(end_of_interrupt, 0);
    }
}
