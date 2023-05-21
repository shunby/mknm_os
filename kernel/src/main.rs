#![no_std]
#![no_main] // もう後戻りできない感じがして興奮する

mod frame_buffer;

use core::mem::{size_of};
use core::panic::PanicInfo;
use core::arch::asm;

use frame_buffer::FrameBufferConfig;

use crate::frame_buffer::PixelFormat;


type PixelColor = (u8,u8,u8);

const FONT_A:  [u8; 16] = [
    0b00000000, //
    0b00011000, //    **
    0b00011000, //    **
    0b00011000, //    **
    0b00011000, //    **
    0b00100100, //   *  *
    0b00100100, //   *  *
    0b00100100, //   *  *
    0b00100100, //   *  *
    0b01111110, //  ******
    0b01000010, //  *    *
    0b01000010, //  *    *
    0b01000010, //  *    *
    0b11100111, // ***  ***
    0b00000000, //
    0b00000000, //
];

pub trait PixelWriter {
    fn write(&self, x: u32, y: u32, color: PixelColor);
}

pub struct RGBPixelWriter<'a> {
    fb_conf: &'a FrameBufferConfig
}

impl<'a> PixelWriter for RGBPixelWriter<'a> {
    fn write(&self, x: u32, y: u32, color: PixelColor) {
        let fb_conf = self.fb_conf;
        
        if x >= fb_conf.horizontal_resolution || y >= fb_conf.vertical_resolution {return;}

        let pixel_position = fb_conf.pixels_per_scanline * y + x;
        unsafe {
            let p = fb_conf.frame_buffer.offset(4 * pixel_position as isize);
            *p = color.0;
            *(p.offset(1)) = color.1;
            *(p.offset(2)) = color.2;
        }
    }
}

pub struct BGRPixelWriter<'a> {
    fb_conf: &'a FrameBufferConfig
}

impl<'a> PixelWriter for BGRPixelWriter<'a> {
    fn write(&self, x: u32, y: u32, color: PixelColor) {
        let fb_conf = self.fb_conf;
        
        if x >= fb_conf.horizontal_resolution || y >= fb_conf.vertical_resolution {return;}
        
        let pixel_position = fb_conf.pixels_per_scanline * y + x;
        unsafe {
            let p = fb_conf.frame_buffer.offset(4 * pixel_position as isize);
            *p = color.2;
            *(p.offset(1)) = color.1;
            *(p.offset(2)) = color.0;
        }
    }
}

pub fn write_ascii(writer: &dyn PixelWriter, x: u32, y: u32, c: char, color: PixelColor) {
    if c != 'A' {return};

    for dy in 0..16u32 {
        for dx in 0..8u32 {
            if ((FONT_A[dy as usize] << dx) & 0b10000000) != 0 {
                writer.write(x+dx, y+dy, color);
            }
        }
    }
}
pub fn new_pixelwriter<'a>(buf: &mut [u8], fb_conf: &'a FrameBufferConfig) -> &'a dyn PixelWriter {
    match fb_conf.pixel_format {
        PixelFormat::PixelBGRResv8BitPerColor => {
            assert!(buf.len() >= size_of::<BGRPixelWriter>());
            unsafe {
                *(buf.as_ptr() as *mut BGRPixelWriter) = BGRPixelWriter {fb_conf};
                &*(buf.as_ptr() as *mut BGRPixelWriter)
            }
        }
        PixelFormat::PixelRGBResv8BitPerColor => {
            assert!(buf.len() >= size_of::<RGBPixelWriter>());
            unsafe {
                *(buf.as_ptr() as *mut RGBPixelWriter) = RGBPixelWriter {fb_conf};
                &*(buf.as_ptr() as *mut RGBPixelWriter)
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn KernelMain(fb_conf: FrameBufferConfig) -> ! {
    let mut pixelwriter_buf = [0u8; size_of::<RGBPixelWriter>()];
    let pixelwriter = new_pixelwriter(&mut pixelwriter_buf, &fb_conf);

    for x in 0..fb_conf.horizontal_resolution {
        for y in 0..fb_conf.vertical_resolution {
            pixelwriter.write(x, y, (1,1,1));
        }
    }
    
    write_ascii(pixelwriter, 50, 50, 'A', (128,128,128));
    write_ascii(pixelwriter, 50, 200, 'A', (255,255,255));

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