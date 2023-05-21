#![no_std]
#![no_main] // もう後戻りできない感じがして興奮する

mod frame_buffer;
#[macro_use]
mod font;
mod graphics;
mod console;

use core::mem::{size_of};
use core::panic::PanicInfo;
use core::arch::asm;

use console::Console;
use font::{IntoU8s};
use frame_buffer::FrameBufferConfig;
use graphics::{new_pixelwriter, RGBPixelWriter};

#[no_mangle]
pub extern "C" fn KernelMain(fb_conf: FrameBufferConfig) -> ! {
    let mut pixelwriter_buf = [0u8; size_of::<RGBPixelWriter>()];
    let pixelwriter = new_pixelwriter(&mut pixelwriter_buf, &fb_conf);

    for x in 0..fb_conf.horizontal_resolution {
        for y in 0..fb_conf.vertical_resolution {
            pixelwriter.write(x, y, (1,1,1));
        }
    }

    let mut console = Console::new(pixelwriter, (255,255,255), (0,0,0));
    
    for i in 0..30 {
        print!(console, "line ", (i+1) as usize, "\n");
    }

    unsafe {
        loop {
            asm!("hlt");
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}