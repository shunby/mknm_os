use core::slice::from_raw_parts_mut;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum PixelFormat {
    PixelRGBResv8BitPerColor,
    PixelBGRResv8BitPerColor
}

pub struct FrameBuffer {
    pub frame_buffer: &'static mut [u8],
    pub pixels_per_scanline: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixel_format: PixelFormat,
}

impl FrameBuffer {
    pub unsafe fn new(raw: *const FrameBufferRaw) -> Self{
        let raw = &*raw;
        let len = (raw.pixels_per_scanline * raw.vertical_resolution * 4) as usize;
        FrameBuffer {
            frame_buffer: unsafe { from_raw_parts_mut(raw.buf, len) },
            pixels_per_scanline: raw.pixels_per_scanline,
            horizontal_resolution: raw.horizontal_resolution,
            vertical_resolution: raw.vertical_resolution,
            pixel_format: raw.pixel_format,
        }
    }
}

#[repr(C)]
pub struct FrameBufferRaw {
    pub buf: *mut u8,
    pub pixels_per_scanline: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixel_format: PixelFormat,
}