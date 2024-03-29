use core::iter::repeat_with;

use alloc::{sync::Arc, vec::Vec};

use crate::memory_manager::{Mutex, RwLock};
use super::{buffered::BufferedCanvas, frame_buffer::FrameBuffer, graphics::{PixelColor, PixelWriter, Rect, Vec2}};
pub struct Window {
    pos: Vec2<i32>,
    width: usize,
    height: usize,
    transparant_color: Option<PixelColor>,
    buffer: BufferedCanvas
}

impl Window {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            pos: (0,0).into(),
            width,
            height,
            buffer: BufferedCanvas::new(width, height),
            transparant_color: None,
        }
    }

    pub fn set_transparent_color(&mut self, color: Option<PixelColor>) {
        self.transparant_color = color;
    }

    #[inline]
    pub fn is_inside(&self, pos: Vec2<i32>) -> bool {
        0 <= pos.x && pos.x < self.width as i32 && 0 <= pos.y && pos.y < self.height as i32
    }

    pub fn draw_to(&self, buf: &mut FrameBuffer) {
        self.buffer.with_fore(|fore|{
            match self.transparant_color {
                None => {
                    buf.copy(self.pos, fore)
                }
                Some(tc) => {
                    let r_window = Rect::from_wh(self.pos.x, self.pos.y, self.width as i32, self.height as i32);
                    let r_fb = Rect::from_wh(0,0,buf.resolution().0 as i32, buf.resolution().1 as i32);
                    let r_draw = match r_fb.intersection(&r_window).map(|r|r.move_relative(-self.pos.x, -self.pos.y)) {
                        None => return,
                        Some(r) => r
                    }; 

                    for y in r_draw.y1 as usize..r_draw.y2 as usize {
                        for x in r_draw.x1 as usize..r_draw.x2 as usize {
                            let pixel = fore.color_at(x, y);
                            if pixel != tc {
                                buf.write(self.pos + (x as i32, y as i32).into(), pixel);
                            }
                        }
                    }
                }
            }
        });
    }

    pub fn move_to(&mut self, pos: Vec2<i32>) -> &mut Self {
        self.pos = pos;
        self
    }

    pub fn move_relative(&mut self, pos_diff: Vec2<i32>) -> &mut Self {
        self.pos = self.pos + pos_diff;
        self
    }

    pub fn move_rect(&self, to: Vec2<i32>, rect: Rect) {
        self.buffer.write_with(|back|{
            back.move_rect(to, rect);
        });
    }

    pub fn pos(&self) -> Vec2<i32> {
        self.pos
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn buffer(&self) -> &BufferedCanvas {
        &self.buffer
    }
}

pub type LayerId = usize;

pub struct LayerHandle {
    window: Arc<RwLock<Window>>,
    layer_id: LayerId
}

impl LayerHandle { 
    pub fn layer_id(&self) -> LayerId {
        self.layer_id
    }

    pub fn window(&self) -> &Arc<RwLock<Window>> {
        &self.window
    }
}

/// 複数のウィンドウを層状に並べて管理・描画する
pub struct LayeredWindowManager {
    layers: Vec<Arc<RwLock<Window>>>,
    layer_stack: Vec<LayerId>,
    buffer: FrameBuffer
}

impl LayeredWindowManager {
    pub fn new(buffer: FrameBuffer) -> Self {
        Self {
            layers: Vec::new(),
            layer_stack: Vec::new(),
            buffer
        }
    }

    pub fn new_layer(&mut self, window: Window) -> LayerHandle {
        let arc = Arc::new(RwLock::new(window));
        self.layers.push(arc.clone());
        LayerHandle { layer_id: self.layers.len()-1, window: arc}
    }

    pub fn move_to(&mut self, id: LayerId, pos: Vec2<i32>) {
        self.layers[id].write().move_to(pos);
    }

    pub fn move_relative(&mut self, id: LayerId, pos_diff: Vec2<i32>) {
        self.layers[id].write().move_relative(pos_diff);
    }

    pub fn draw(&mut self) {
        for id in &self.layer_stack {
            self.layers[*id].read().draw_to(&mut self.buffer);
        }
    }

    pub fn hide(&mut self, id: LayerId) {
        self.layer_stack.retain(|lid| *lid != id);
    }

    pub fn up_down(&mut self, id: usize, new_height: i32) {
        if new_height < 0 {
            self.hide(id);
            return;
        }

        let new_height = (new_height as usize).min(self.layer_stack.len());
        match self.layer_stack.iter().find(|&&lid| lid == id) {
            None => {
                self.layer_stack.insert(
                    new_height,
                    id
                );
            }
            Some(layer) => {
                let layer = layer.clone();
                self.hide(id);
                let new_height = new_height.min(self.layer_stack.len());
                self.layer_stack.insert(new_height, layer);
            }
        }
    }

    pub fn resolution(&self) -> (u32, u32) {
        self.buffer.resolution()
    }
}
