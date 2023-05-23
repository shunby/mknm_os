#![no_std]
#![no_main] // もう後戻りできない感じがして興奮する

mod frame_buffer;
#[macro_use]
mod font;
mod graphics;
mod console;
mod pci;
mod mouse;

use core::mem::{size_of, MaybeUninit, transmute};
use core::panic::PanicInfo;
use core::arch::asm;

use console::Console;
use frame_buffer::FrameBufferConfig;
use graphics::{new_pixelwriter, RGBPixelWriter, draw_bitpattern, Vec2};
use mouse::MouseCursor;
use pci::{PCIController, PCIDevice};

use usb_bindings::raw::{usb_xhci_ConfigurePort, usb_xhci_ProcessEvent, usb_set_default_mouse_observer};


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
struct Peripheral<'a> {
    console: Console<'a>,
    mouse: MouseCursor<'a>
}
static mut PERIPHERAL: MaybeUninit<Peripheral> = MaybeUninit::uninit();
static mut IS_PERIPHERAL_INITIALIZED: bool = false;
fn get_console() -> &'static mut Console<'static> {
    unsafe {
        if !IS_PERIPHERAL_INITIALIZED {panic!()}
        &mut PERIPHERAL.assume_init_mut().console
    }
}

fn get_mouse() -> &'static mut MouseCursor<'static> {
    unsafe {
        if !IS_PERIPHERAL_INITIALIZED {panic!()}
        &mut PERIPHERAL.assume_init_mut().mouse
    }
}

fn init_peripheral(periph: Peripheral){
    unsafe {
        if IS_PERIPHERAL_INITIALIZED {panic!()}
        PERIPHERAL = MaybeUninit::new(transmute::<_, Peripheral<'static>>(periph));
        IS_PERIPHERAL_INITIALIZED = true;
    }
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

fn mouse_event_loop() {
    unsafe {
        let xhc = find_xhc_device();
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
        print!("entering main loop\n");

        loop {
            let err = usb_xhci_ProcessEvent(&mut xhc);
            if err.code_ != 0 {
                print!("error while processevent: ", err.code_, "\n");
            }
        }
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

#[no_mangle]
pub extern "C" fn KernelMain(fb_conf: FrameBufferConfig) -> ! {
    let mut pixelwriter_buf = [0u8; size_of::<RGBPixelWriter>()];
    let pixelwriter = new_pixelwriter(&mut pixelwriter_buf, &fb_conf);

    for x in 0..fb_conf.horizontal_resolution {
        for y in 0..fb_conf.vertical_resolution {
            pixelwriter.write(x, y, (100,100,100));
        }
    }

    init_peripheral(
        Peripheral { 
            console: Console::new(pixelwriter, (255,255,255), (100,100,100)),
            mouse: MouseCursor::new(pixelwriter, (100,100,100), Vec2::new(200,300))
        }
    );

    unsafe{
        usb_bindings::raw::SetLogLevel(1);
        usb_bindings::raw::SetPrintFn(Some(print_c));
    }
    draw_bitpattern(pixelwriter, Vec2{x:100u32,y:100u32}, &LOGO, (0,0,255), 5);
    scan_pci_devices();
    mouse_event_loop();
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