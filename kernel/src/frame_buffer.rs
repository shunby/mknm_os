use core::{slice::{from_raw_parts_mut}};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum PixelFormat {
    PixelRGBResv8BitPerColor,
    PixelBGRResv8BitPerColor
}

pub struct FrameBuffer<'a> {
    pub frame_buffer: &'a mut [u8],
    pub pixels_per_scanline: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixel_format: PixelFormat,
}

#[repr(C)]
pub struct FrameBufferRaw {
    pub buf: *mut u8,
    pub pixels_per_scanline: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixel_format: PixelFormat,
}

impl<'a> Into<FrameBuffer<'a>> for &FrameBufferRaw {
    fn into(self) -> FrameBuffer<'a> {
        let len = (self.pixels_per_scanline * self.vertical_resolution * 4) as usize;
        FrameBuffer {
            frame_buffer: unsafe { from_raw_parts_mut(self.buf, len) },
            pixels_per_scanline: self.pixels_per_scanline,
            horizontal_resolution: self.horizontal_resolution,
            vertical_resolution: self.vertical_resolution,
            pixel_format: self.pixel_format,
        }
    }
}
