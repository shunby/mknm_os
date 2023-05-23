#![no_std]
#![no_main] // もう後戻りできない感じがして興奮する

mod frame_buffer;
#[macro_use]
mod font;
mod graphics;
mod console;
mod pci;

use core::mem::{size_of, MaybeUninit, transmute};
use core::panic::PanicInfo;
use core::arch::asm;

use console::Console;
use frame_buffer::FrameBufferConfig;
use graphics::{new_pixelwriter, RGBPixelWriter, draw_bitpattern, Vec2};
use pci::{PCIController, PCIDevice};

const MOUSE_CURSOR_DIMENSION: (usize, usize) = (15, 24);
const MOUSE_CURSOR_SHAPE: [&'static str; MOUSE_CURSOR_DIMENSION.1] = [
    "@              ",
    "@@             ",
    "@.@            ",
    "@..@           ",
    "@...@          ",
    "@....@         ",
    "@.....@        ",
    "@......@       ",
    "@.......@      ",
    "@........@     ",
    "@.........@    ",
    "@..........@   ",
    "@...........@  ",
    "@............@ ",
    "@......@@@@@@@@",
    "@......@       ",
    "@....@@.@      ",
    "@...@ @.@      ",
    "@..@   @.@     ",
    "@.@    @.@     ",
    "@@      @.@    ",
    "@       @.@    ",
    "@        @.@   ",
    "         @@@   ",
];

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
static mut console: MaybeUninit<Console> = MaybeUninit::uninit();
static mut is_console_initialized: bool = false;
fn get_console() -> &'static mut Console<'static> {
    unsafe {
        if !is_console_initialized {panic!()}
        console.assume_init_mut()
    }
}

fn init_console(consol: Console){
    unsafe {
        if is_console_initialized {panic!()}
        console = MaybeUninit::new(transmute::<_, Console<'static>>(consol));
        is_console_initialized = true;
    }
}

fn scan_pci_devices() {
    let mut pci = PCIController::new();
    unsafe {pci.scan_all_bus().unwrap();}

    let devices = pci.get_devices();
    
    for i in 0..pci.num_devices() {
        let dev: &PCIDevice = unsafe{devices[i].assume_init_ref()};
        let index = dev.get_index();
        print!(index.0, ".", index.1, ".", index.2, ": head ", unsafe {dev.read_header_type()}, "\n");        
    }

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

    init_console(Console::new(pixelwriter, (255,255,255), (100,100,100)));
    
    for i in 0..30 {
        print!("line ", (i+1) as usize, "\n");
    }
    
    scan_pci_devices();

    for dy in 0..MOUSE_CURSOR_DIMENSION.1 {
        for dx in 0..MOUSE_CURSOR_DIMENSION.0 {
            match MOUSE_CURSOR_SHAPE[dy].as_bytes()[dx] as char {
                '.' => pixelwriter.write(200+dx as u32, 200+dy as u32, (255,255,255)),
                '@' => pixelwriter.write(200+dx as u32, 200+dy as u32, (0,0,0)),
                _ => ()
            }
        }
    }

    draw_bitpattern(pixelwriter, Vec2{x:100u32,y:100u32}, &LOGO, (0,0,255), 5);

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