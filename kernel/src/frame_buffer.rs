use core::slice::from_raw_parts_mut;

use alloc::vec::Vec;

use crate::{
    graphic::graphics::{PixelWriter, Rect, Vec2},
    memory_manager::Mutex,
};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum PixelFormat {
    PixelRGBResv8BitPerColor,
    PixelBGRResv8BitPerColor,
}

impl PixelFormat {
    #[inline]
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::PixelBGRResv8BitPerColor => 4,
            PixelFormat::PixelRGBResv8BitPerColor => 4,
        }
    }

    pub fn write(self, color: (u8, u8, u8), buffer: &mut [u8]) {
        match self {
            PixelFormat::PixelBGRResv8BitPerColor => {
                buffer[0] = color.2;
                buffer[1] = color.1;
                buffer[2] = color.0;
            }
            PixelFormat::PixelRGBResv8BitPerColor => {
                buffer[0] = color.0;
                buffer[1] = color.1;
                buffer[2] = color.2;
            }
        }
    }
}

enum FrameBufferData {
    Vram(&'static mut [u8]),
    Shadow(Vec<u8>),
}

impl FrameBufferData {
    pub fn get_mut(&mut self) -> &mut [u8] {
        match self {
            FrameBufferData::Vram(vram) => vram,
            FrameBufferData::Shadow(buf) => buf,
        }
    }

    pub fn get(&self) -> &[u8] {
        match self {
            FrameBufferData::Vram(vram) => vram,
            FrameBufferData::Shadow(buf) => buf,
        }
    }
}

pub struct FrameBufferConf {
    pixels_per_scanline: u32,
    horizontal_resolution: u32,
    vertical_resolution: u32,
    pixel_format: PixelFormat,
}

impl FrameBufferConf {
    fn to_index(&self, x: i32, y: i32) -> usize {
        (y as usize * self.pixels_per_scanline as usize + x as usize)
            * self.pixel_format.bytes_per_pixel()
    }
}

pub struct FrameBuffer {
    data: FrameBufferData,
    conf: FrameBufferConf,
}

static DEFAULT_PIXEL_FORMAT: Mutex<Option<PixelFormat>> = Mutex::new(None);

pub fn set_default_pixel_format(format: PixelFormat) {
    *DEFAULT_PIXEL_FORMAT.lock() = Some(format);
}

impl FrameBuffer {
    pub unsafe fn from_raw(raw: *const FrameBufferRaw) -> Self {
        let raw = &*raw;
        let len = (raw.pixels_per_scanline * raw.vertical_resolution * 4) as usize;
        FrameBuffer {
            data: FrameBufferData::Vram(from_raw_parts_mut(raw.buf, len)),
            conf: FrameBufferConf {
                pixels_per_scanline: raw.pixels_per_scanline,
                horizontal_resolution: raw.horizontal_resolution,
                vertical_resolution: raw.vertical_resolution,
                pixel_format: raw.pixel_format,
            },
        }
    }

    pub fn new(width: usize, height: usize) -> Self {
        let format = DEFAULT_PIXEL_FORMAT.lock().unwrap();
        let data = vec![0u8; width * height * format.bytes_per_pixel()];
        FrameBuffer {
            data: FrameBufferData::Shadow(data),
            conf: FrameBufferConf {
                pixels_per_scanline: width as u32,
                horizontal_resolution: width as u32,
                vertical_resolution: height as u32,
                pixel_format: format,
            },
        }
    }
    pub fn pixels_per_scanline(&self) -> u32 {
        self.conf.pixels_per_scanline
    }

    pub fn resolution(&self) -> (u32, u32) {
        (
            self.conf.horizontal_resolution,
            self.conf.vertical_resolution,
        )
    }

    pub fn pixel_format(&self) -> PixelFormat {
        self.conf.pixel_format
    }

    pub fn copy(&mut self, pos: Vec2<i32>, from: &FrameBuffer) {
        let rect_buf = Rect::from_wh(
            0,
            0,
            self.conf.horizontal_resolution as i32,
            self.conf.vertical_resolution as i32,
        );
        let rect_from = Rect::from_wh(
            pos.x,
            pos.y,
            from.conf.horizontal_resolution as i32,
            from.conf.vertical_resolution as i32,
        );

        let modified_rect = match rect_buf.intersection(&rect_from) {
            None => {
                return;
            }
            Some(r) => r,
        };

        let copied_rect = modified_rect.move_relative(-pos.x, -pos.y);

        let buf_to = self.data.get_mut();
        let buf_from = from.data.get();

        for (y_to, y_from) in
            (modified_rect.y1..modified_rect.y2).zip(copied_rect.y1..copied_rect.y2)
        {
            let xs_to = (
                self.conf.to_index(modified_rect.x1, y_to),
                self.conf.to_index(modified_rect.x2, y_to),
            );
            let xs_from = (
                from.conf.to_index(copied_rect.x1, y_from),
                from.conf.to_index(copied_rect.x2, y_from),
            );
            buf_to[xs_to.0..xs_to.1].copy_from_slice(&buf_from[xs_from.0..xs_from.1]);
        }
    }

    pub fn move_rect(&mut self, to: Vec2<i32>, rect: Rect) {
        assert!(rect.contained_by(&Rect::from_wh(0, 0, self.conf.horizontal_resolution as i32, self.conf.vertical_resolution as i32)));
        let buf = self.data.get_mut();

        if to.y <= rect.y1 {
            for y in 0..(rect.y2 - rect.y1) {
                let xs_from = (
                    self.conf.to_index(rect.x1, y+rect.y1),
                    self.conf.to_index(rect.x2, y+rect.y1)
                );
                let xs_to = (
                    self.conf.to_index(to.x, y+to.y),
                    self.conf.to_index(to.x + rect.x2 - rect.x1, y + to.y)
                );
                buf.copy_within(xs_from.0..xs_from.1, xs_to.0);
            }
        } else {
            for y in (0..(rect.y2 - rect.y1)).rev() {
                let xs_from = (
                    self.conf.to_index(rect.x1, y+rect.y1),
                    self.conf.to_index(rect.x2, y+rect.y1)
                );
                let xs_to = (
                    self.conf.to_index(to.x, y+to.y),
                    self.conf.to_index(to.x + rect.x2 - rect.x1, y + to.y)
                );
                buf.copy_within(xs_from.0..xs_from.1, xs_to.0);
            }
        }
    }
}

impl PixelWriter for FrameBuffer {
    fn write(&mut self, pos: crate::graphic::graphics::Vec2<i32>, color: crate::graphic::graphics::PixelColor) {
        let i_pixel: usize =
            self.conf.pixels_per_scanline as usize * pos.y as usize + pos.x as usize;
        match &mut self.data {
            FrameBufferData::Vram(vram) => {
                self.conf.pixel_format.write(
                    color,
                    &mut vram[i_pixel * self.conf.pixel_format.bytes_per_pixel()..],
                );
            }
            FrameBufferData::Shadow(buf) => self.conf.pixel_format.write(
                color,
                &mut buf[i_pixel * self.conf.pixel_format.bytes_per_pixel()..],
            ),
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
