use core::{mem::size_of, ops::Add};

use crate::frame_buffer::{FrameBufferConfig, PixelFormat};



pub type PixelColor = (u8,u8,u8);

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

#[derive(Debug)]
pub struct Vec2<T>{
    pub x: T,
    pub y: T
}

impl<T> Vec2<T> {
    pub fn new(x: T, y: T) -> Self {
        Self {x,y}
    }
}

impl<T> Add<&Vec2<T>> for &Vec2<T> where for<'a, 'b> &'a T: Add<&'b T, Output = T>{
    type Output = Vec2<T>;
    fn add(self, rhs: &Vec2<T>) -> Self::Output {
        Vec2 {
            x: (&self.x) + (&rhs.x),
            y: (&self.y) + (&rhs.y) 
        }
    }
}

pub fn fill_rect(writer: &dyn PixelWriter, pos: Vec2<u32>, size: Vec2<u32>, c: PixelColor) {
    for x in pos.x..pos.x + size.x {
        for y in pos.y..pos.y + size.y {
            writer.write(x, y, c);
        }
    }
}

pub fn draw_bitpattern<const N: usize>(writer: &dyn PixelWriter, pos: Vec2<u32>, pattern: &[u64;N], c: PixelColor, scale: u8) {
    for dy in 0..N {
        for dx in 0..64 {
            if (pattern[dy] >> (63-dx)) & 1 == 1 {
                fill_rect(
                    writer, 
                    &pos + &Vec2::new((scale*dx) as u32, (scale * dy as u8) as u32),
                    Vec2::new(scale as u32, scale as u32), 
                    c
                );
            }
        }
    }
}