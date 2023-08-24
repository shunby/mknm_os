#[repr(C)]
#[derive(Debug)]
pub enum PixelFormat {
    PixelRGBResv8BitPerColor,
    PixelBGRResv8BitPerColor
}

#[repr(C)]
#[derive(Debug)]
pub struct FrameBufferConfig{
    pub frame_buffer: *mut u8,
    pub pixels_per_scanline : u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixel_format: PixelFormat
}