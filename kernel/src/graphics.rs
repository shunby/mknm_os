use core::ops::Add;

use crate::frame_buffer::{PixelFormat, FrameBuffer};

pub type PixelColor = (u8,u8,u8);


fn write_bgr(fb: &mut FrameBuffer, pos: Vec2<u32>, color: PixelColor) {
    let (x,y) = (pos.x, pos.y);
    if x >= fb.horizontal_resolution || y >= fb.vertical_resolution {return;}
    
    let pixel_position = ((fb.pixels_per_scanline * y + x) * 4) as usize;
    
    fb.frame_buffer[pixel_position] = color.2;
    fb.frame_buffer[pixel_position+1] = color.1;
    fb.frame_buffer[pixel_position+2] = color.0;
}

fn write_rgb(fb: &mut FrameBuffer, pos: Vec2<u32>, color: PixelColor) {
    let (x,y) = (pos.x, pos.y);
    if x >= fb.horizontal_resolution || y >= fb.vertical_resolution {return;}
    
    let pixel_position = ((fb.pixels_per_scanline * y + x) * 4) as usize;
    
    fb.frame_buffer[pixel_position] = color.0;
    fb.frame_buffer[pixel_position+1] = color.1;
    fb.frame_buffer[pixel_position+2] = color.2;
}

pub struct Graphics {
    writer: fn(&mut FrameBuffer, Vec2<u32>, PixelColor),
    fb: FrameBuffer
}

impl Graphics {
    pub fn new(fb: FrameBuffer) -> Self {
        Self {
            writer: match fb.pixel_format {
                PixelFormat::PixelBGRResv8BitPerColor => write_bgr,
                PixelFormat::PixelRGBResv8BitPerColor => write_rgb
            }, 
            fb
        }
    }

    pub fn write_pixel(&mut self, pos: Vec2<u32>, c: PixelColor) {
        (self.writer)(&mut self.fb, pos, c);
    }

    pub fn fill_rect(&mut self, pos: Vec2<u32>, size: Vec2<u32>, c: PixelColor) {
        for x in pos.x..pos.x + size.x {
            for y in pos.y..pos.y + size.y {
                self.write_pixel(&pos + &Vec2::new(x,y), c);
            }
        }
    }

    pub fn draw_bitpattern<const N: usize>(&mut self, pos: Vec2<u32>, pattern: &[u64;N], c: PixelColor, scale: u32) {
        for dy in 0..N {
            for dx in 0usize..64 {
                if (pattern[dy] >> (63-dx)) & 1 == 1 {
                    self.fill_rect(
                        &pos + &Vec2::new(scale*dx as u32, scale * dy as u32),
                        Vec2::new(scale, scale), 
                        c
                    );
                }
            }
        }
    }
    
    pub fn pixels_per_scanline(&self) -> u32 {
        self.fb.pixels_per_scanline
    }

    pub fn resolution(&self) -> (u32, u32) {
        (self.fb.horizontal_resolution, self.fb.vertical_resolution)
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

impl<T> Into<Vec2<T>> for (T,T) {
    fn into(self) -> Vec2<T> {
        Vec2 { x: self.0, y: self.1 }
    }
}
