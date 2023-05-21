#![no_std]
#![no_main] // もう後戻りできない感じがして興奮する

mod frame_buffer;
mod font;
mod graphics;

use core::mem::{size_of};
use core::panic::PanicInfo;
use core::arch::asm;

use font::write_ascii;
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

    for i in 0..26 {
        write_ascii(pixelwriter, 50 + 8 * i, 50, ('A' as u8 + i as u8) as char, (128,128,128));
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