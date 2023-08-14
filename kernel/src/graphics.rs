use core::ops::Add;

use alloc::boxed::Box;

use crate::frame_buffer::{PixelFormat, FrameBuffer};

pub type PixelColor = (u8,u8,u8);

pub trait PixelWriter {
    fn write(&mut self, pos: Vec2<i32>, color: PixelColor);

    fn fill_rect(&mut self, pos: Vec2<i32>, size: Vec2<u32>, c: PixelColor) {
        for x in pos.x..pos.x + size.x as i32 {
            for y in pos.y..pos.y + size.y as i32 {
                self.write(&pos + &Vec2::new(x,y), c);
            }
        }
    }

    fn draw_bitpattern(&mut self, pos: Vec2<i32>, pattern: &[u64], c: PixelColor, scale: u32) {
        for dy in 0..pattern.len() {
            for dx in 0usize..64 {
                if (pattern[dy] >> (63-dx)) & 1 == 1 {
                    self.fill_rect(
                        &pos + &Vec2::new((scale*dx as u32) as i32, (scale * dy as u32) as i32),
                        Vec2::new(scale, scale), 
                        c
                    );
                }
            }
        }
    }
}

fn write_bgr(fb: &mut FrameBuffer, pos: Vec2<i32>, color: PixelColor) {
    let (x,y) = (pos.x, pos.y);
    if x >= fb.horizontal_resolution as i32 || x < 0 || y >= fb.vertical_resolution as i32 || y < 0 {return;}
    
    let pixel_position = ((fb.pixels_per_scanline as i32 * y + x) * 4) as usize;
    
    fb.frame_buffer[pixel_position] = color.2;
    fb.frame_buffer[pixel_position+1] = color.1;
    fb.frame_buffer[pixel_position+2] = color.0;
}

fn write_rgb(fb: &mut FrameBuffer, pos: Vec2<i32>, color: PixelColor) {
    let (x,y) = (pos.x, pos.y);
    if x >= fb.horizontal_resolution as i32 || x < 0 || y >= fb.vertical_resolution as i32 || y < 0 {return;}
    
    let pixel_position = ((fb.pixels_per_scanline as i32 * y + x) * 4) as usize;
    
    fb.frame_buffer[pixel_position] = color.0;
    fb.frame_buffer[pixel_position+1] = color.1;
    fb.frame_buffer[pixel_position+2] = color.2;
}

pub struct Graphics {
    writer: fn(&mut FrameBuffer, Vec2<i32>, PixelColor),
    fb: FrameBuffer
}

impl PixelWriter for Graphics {
    fn write(&mut self, pos: Vec2<i32>, color: PixelColor) {
        (self.writer)(&mut self.fb, pos, color);
    }
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

    pub fn pixels_per_scanline(&self) -> u32 {
        self.fb.pixels_per_scanline
    }

    pub fn resolution(&self) -> (u32, u32) {
        (self.fb.horizontal_resolution, self.fb.vertical_resolution)
    }
}

#[derive(Debug, Clone, Copy)]
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


impl<T> Add<Vec2<T>> for Vec2<T> where T: Add<T, Output = T>{
    type Output = Vec2<T>;
    fn add(self, rhs: Vec2<T>) -> Self::Output {
        Vec2 {
            x: (self.x) + (rhs.x),
            y: (self.y) + (rhs.y) 
        }
    }
}

impl<T> From<(T,T)> for Vec2<T> {
    fn from(value: (T,T)) -> Self {
        Vec2 { x: value.0, y: value.1 }
    }
}
