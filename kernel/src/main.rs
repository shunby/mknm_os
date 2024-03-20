#![cfg_attr(not(test), no_std)]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(allocator_api)]

mod graphic;
#[macro_use]
mod console;
mod pci;
mod mouse;
mod interrupt;
mod memory_map;
mod memory_manager;
mod segment;
mod paging;
mod acpi;
mod timer;
mod usb;
mod asm;
mod task;
mod taskB;

#[macro_use]
extern crate alloc;

use core::alloc::Layout;
use core::mem::transmute;
use core::panic::PanicInfo;
use core::arch::{asm, global_asm};
use core::ptr::write_volatile;
use core::str::from_utf8;
use core::sync::atomic::{AtomicU64, Ordering};

use acpi::RSDP;
use alloc::collections::VecDeque;
use alloc::string::ToString;
use alloc::boxed::Box;
use console::Console;
use graphic::frame_buffer::FrameBufferRaw;
use graphic::graphics::PixelWriter;
use graphic::with_layers;
use interrupt::{set_idt_entry, IVIndex, InterruptDescriptor, InterruptDescriptorAttribute, DescriptorType, load_idt};
use memory_manager::LazyInit;
use memory_map::{MemoryMapRaw, MemoryMap};
use pci::{PCIController, PCIDevice, configure_msi_fixed_destination};

use graphic::window::LayeredWindowManager;

use crate::asm::get_cr3;
use crate::console::init_console;
use crate::graphic::font::{write_ascii, write_string};
use crate::graphic::graphics::Vec2;
use crate::interrupt::set_interrupt_flag;
use crate::memory_manager::init_allocators;
use crate::mouse::draw_cursor;
use crate::paging::setup_identity_page_table;
use crate::segment::{setup_segments, KERNEL_CS, KERNEL_SS};
use crate::task::{switch_context, TaskContext};
use crate::timer::{add_timer, get_current_tick, initialize_timer};
use crate::usb::init_usb;
use crate::usb::xhci::initialize_xhci;
use crate::graphic::window::Window;


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

static EVENTS: LazyInit<MessageQueue<1024>> = LazyInit::new();

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
pub unsafe extern "sysv64" fn KernelMain(fb: *const FrameBufferRaw, mm: *const MemoryMapRaw, rsdp: *const RSDP) -> ! {
    unsafe { 
        asm!("lea rsp, [kernel_main_stack + 1024 * 1024]");
        KernelMain2(fb, mm, rsdp);
        asm!(
            "   hlt",
            "   jmp .fin"
        );
    }
}

pub fn draw_window(window: &mut Window, title: &[u8]) {
    let win_h = window.height() as u32;
    let win_w = window.width() as u32;
    window.fill_rect((0,0).into(), (win_w,1).into(), (0xc6,0xc6,0xc6));
    window.fill_rect((1,1).into(), (win_w-2,1).into(), (0xff,0xff,0xff));
    window.fill_rect((0,0).into(), (1, win_h).into(), (0xc6,0xc6,0xc6));
    window.fill_rect((1,1).into(), (1, win_h-2).into(), (0xff,0xff,0xff));
    window.fill_rect((win_w as i32 - 2,1).into(), (1, win_h-2).into(), (0x84,0x84,0x84));
    window.fill_rect((win_w as i32 - 1,0).into(), (1, win_h).into(), (0x00,0x00,0x00));
    window.fill_rect((2, 2).into(), (win_w-4, win_h-4).into(), (0xc6,0xc6,0xc6));
    window.fill_rect((3, 3).into(), (win_w-6, 18).into(), (0x00,0x00,0x84));
    window.fill_rect((1, win_h as i32 - 2).into(), (win_w-2, 1).into(), (0x84,0x84,0x84));
    window.fill_rect((0, win_h as i32 - 1).into(), (win_w, 1).into(), (0x00,0x00,0x00));
    
    write_string(window, 24, 4, title, (0xff,0xff,0xff));

}

unsafe fn initialize_windows() -> (graphic::window::LayerHandle, graphic::window::LayerHandle) {
    with_layers(|layer_mgr|{
        let mut mouse_window = Window::new(15, 24);
        mouse_window.set_transparent_color(Some((1,1,1)));
        draw_cursor(&mut mouse_window);

        let mouse_window_hndl = layer_mgr.new_layer(mouse_window);
        
        let mut test_window = Window::new(160, 68);
        test_window.move_to((100,200).into());
        write_string(&mut test_window, 24, 28, "Welcome to".as_bytes(), (0,0,0));
        write_string(&mut test_window, 24, 44, "Mikanami world!".as_bytes(), (0,0,0));
        draw_window(&mut test_window, "test window".as_bytes());
        let test_window_hndl = layer_mgr.new_layer(test_window);
        
        layer_mgr.up_down(test_window_hndl.layer_id(), 1);
        layer_mgr.up_down(mouse_window_hndl.layer_id(), 2);
        (mouse_window_hndl, test_window_hndl)
    })
}

pub static mut TASK_A_CTX: TaskContext = TaskContext::new();
pub static mut TASK_B_CTX: TaskContext = TaskContext::new();

#[no_mangle]
pub unsafe extern "sysv64" fn KernelMain2(fb: *const FrameBufferRaw, mm: *const MemoryMapRaw, rsdp: *const RSDP) -> ! {
    let memmap: MemoryMap = (&*mm).into();
    setup_segments();
    setup_identity_page_table();
    init_allocators(&memmap);
    set_interrupt_flag(false);   

    graphic::initialize_winmgr(fb);
    let (mouse_window_hndl, test_window_hndl) = initialize_windows();
    acpi::initialize(&*rsdp);
    initialize_timer();

    init_console((255,255,255), (100,100,100));
    
    let pci = scan_pci_devices();

    EVENTS.lock().init(MessageQueue::new());
    set_idt_entry(
        IVIndex::XHCI, 
        InterruptDescriptor::new(
            get_cs(), 
            InterruptDescriptorAttribute::new(0, DescriptorType::InterruptGate), 
            transmute(xhci_interrupt_handler as *const fn())
        )
    );
    set_idt_entry(
        IVIndex::LapicTimer, 
        InterruptDescriptor::new(
            get_cs(),
            InterruptDescriptorAttribute::new(0, DescriptorType::InterruptGate),
            transmute(lapic_interrupt_handler as *const fn())
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

    init_usb(xhc, intel_ehci_found, Box::new(move |report| {
        {
            let (display_width, display_height) = with_layers(|l|l.resolution());
            let (dx,dy) = (report.dx(), report.dy());
            {
                let mut window = mouse_window_hndl.window().lock();
                let new_pos = (window.pos() + (dx as i32, dy as i32).into()).clamp((0,0).into(), (display_width as i32, display_height as i32).into());
                window.move_to(new_pos);
            }
            with_layers(|l|l.draw());
        }
    }));

    print!("finish\n");
    // LAYERS.lock().draw();
    set_interrupt_flag(true);   

    let task_b_stack = vec![0u64;1024];
    let task_b_stack_end = (&task_b_stack[1023]) as *const u64 as u64 + 8;

    TASK_B_CTX.rip = taskB::taskB as *const fn() as u64;
    TASK_B_CTX.rdi = 1;
    TASK_B_CTX.rsi = 42;

    TASK_B_CTX.cr3 = get_cr3();
    TASK_B_CTX.rflags = 0x202;
    TASK_B_CTX.cs = KERNEL_CS as u64;
    TASK_B_CTX.ss = KERNEL_SS as u64;
    TASK_B_CTX.rsp = (task_b_stack_end & !0xfu64) - 8;
    TASK_B_CTX.fxsave_area[6] = 0x1f80;
    
    add_timer(get_current_tick() + 200, 1);
    add_timer(get_current_tick() + 600, 2);

    loop {
        set_interrupt_flag(false);
        if EVENTS.lock().cnt == 0 && TIMER_ELAPSED.load(Ordering::Relaxed) == 0 {
            set_interrupt_flag(true);
            asm!("hlt"); // 割り込みがあるまで休眠
            continue;
        }

        let elapsed = TIMER_ELAPSED.swap(0, Ordering::Relaxed);
        if elapsed > 0 {
            timer::on_lapic_interrupt(elapsed);
        }


        // println!("{:?}", EVENTS.lock().data);

        let msg = EVENTS.lock().pop();
        set_interrupt_flag(true);

        {
            {
                let mut window = test_window_hndl.window().lock();
                let tick = get_current_tick();
                window.fill_rect((24,28).into(), (8*10,16).into(), (0xc6, 0xc6, 0xc6));
                write_string(&mut *window, 24, 28, tick.to_string().as_bytes(), (0,0,0));
            }
            with_layers(|l|l.draw());
        }

        match msg {
            Some(Message::Xhci) => usb::on_xhc_interrupt(),
            Some(Message::TimerTimeout(val)) => match val {
                1 => {
                    let tick = get_current_tick();
                    println!("tick {}: timer 1", tick);
                    add_timer(tick + 200, 1);
                },
                2 => {
                    let tick = get_current_tick();
                    println!("tick {}: timer 2", tick);
                    add_timer(tick + 600, 2);
                }, 
                _ => ()
            }
            _ => ()
        }

        switch_context(&TASK_B_CTX, &mut TASK_A_CTX);
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

#[derive(Clone, Copy, Debug)]
enum Message {
    Xhci,
    TimerTimeout(u64)
}

struct MessageQueue<const N: usize> {
    data: [Message; N],
    read_pos: usize,
    write_pos: usize,
    cnt: usize
}

impl<const N: usize> MessageQueue<N> {
    fn new() -> Self {
        Self {
            data: [Message::Xhci; N],
            read_pos: 0,
            write_pos: 0,
            cnt: 0
        }
    }

    fn push(&mut self, msg: Message) -> Result<(), ()>{
        if self.cnt == self.data.len() {
            return Err(());
        }

        self.cnt += 1;
        self.data[self.write_pos] = msg;
        self.write_pos = (self.write_pos + 1) % self.data.len();
        Ok(())
    }

    fn pop(&mut self) -> Option<Message>{
        if self.cnt == 0 {
            return None;
        }

        self.cnt -= 1;
        let msg = self.data[self.read_pos];
        self.read_pos = (self.read_pos + 1) % self.data.len();
        Some(msg)
    }
}

#[allow(dead_code)]
extern "x86-interrupt" fn xhci_interrupt_handler() {
    let mut lock = EVENTS.lock();
    let _ = lock.push(Message::Xhci);
    notify_end_of_interrupt();
}

static TIMER_ELAPSED: AtomicU64 = AtomicU64::new(0);
extern "x86-interrupt" fn lapic_interrupt_handler() {
    TIMER_ELAPSED.fetch_add(1, Ordering::Relaxed);
    notify_end_of_interrupt();
}

fn notify_end_of_interrupt() {
    unsafe {
        let end_of_interrupt = 0xfee000b0u64 as *mut u32;
        write_volatile(end_of_interrupt, 0);
    }
}
