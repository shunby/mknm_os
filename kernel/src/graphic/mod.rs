use crate::{ memory_manager::LazyInit};

use self::{frame_buffer::{FrameBuffer, FrameBufferRaw}, window::LayeredWindowManager};

pub mod window;
pub mod font;
pub mod graphics;
pub mod frame_buffer;

static LAYERS: LazyInit<LayeredWindowManager> = LazyInit::new();

pub unsafe fn initialize_winmgr(fb: *const FrameBufferRaw) {
    let mut fb = FrameBuffer::from_raw(fb);
    frame_buffer::set_default_pixel_format(fb.pixel_format());
    LAYERS.lock().init(LayeredWindowManager::new(fb));
}

pub fn with_layers<R>(f: impl FnOnce(&mut LayeredWindowManager) -> R) -> R {
    f(&mut LAYERS.lock())
}
