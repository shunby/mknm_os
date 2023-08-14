use core::iter::repeat_with;

use alloc::vec::Vec;

use crate::{graphics::{PixelColor, Vec2, PixelWriter, Rect}, frame_buffer::FrameBuffer};

pub struct Window {
    pos: Vec2<i32>,
    width: usize,
    height: usize,
    data: Vec<Vec<PixelColor>>,
    transparant_color: Option<PixelColor>,
    shadow: FrameBuffer
}

impl Window {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            pos: (0,0).into(),
            width,
            height,
            data: repeat_with(|| vec![(0, 0, 0); width])
                .take(height)
                .collect(),
            transparant_color: None,
            shadow: FrameBuffer::new(width, height)
        }
    }

    pub fn set_transparent_color(&mut self, color: Option<PixelColor>) {
        self.transparant_color = color;
    }

    #[inline]
    fn get_mut_at(&mut self, pos: Vec2<i32>) -> &mut PixelColor {
        debug_assert!(self.is_inside(pos));
        &mut self.data[pos.y as usize][pos.x as usize]
    }

    #[inline]
    pub fn is_inside(&self, pos: Vec2<i32>) -> bool {
        0 <= pos.x && pos.x < self.width as i32 && 0 <= pos.y && pos.y < self.height as i32
    }

    pub fn draw_to(&self, buf: &mut FrameBuffer) {
        match self.transparant_color {
            None => {
                buf.copy(self.pos, &self.shadow);
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
                        let pixel = &self.data[y][x];
                        if *pixel != tc {
                            buf.write(self.pos + (x as i32, y as i32).into(), *pixel);
                        }
                    }
                }
            }
        }
    }

    pub fn move_to(&mut self, pos: Vec2<i32>) -> &mut Self {
        self.pos = pos;
        self
    }

    pub fn move_relative(&mut self, pos_diff: Vec2<i32>) -> &mut Self {
        self.pos = self.pos + pos_diff;
        self
    }

    pub fn move_rect(&mut self, to: Vec2<i32>, rect: Rect) {
        self.shadow.move_rect(to, rect);
    }
}

impl PixelWriter for Window {
    fn write(&mut self, pos: Vec2<i32>, c: PixelColor) {
        if self.is_inside(pos) {
            *self.get_mut_at(pos) = c;
            self.shadow.write(pos, c);
        }
    }
}

pub type LayerId = usize;

pub struct LayeredWindowManager {
    layers: Vec<Window>,
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

    pub fn with_layer_mut<R>(&mut self, id: LayerId, f: impl Fn(&mut Window) -> R) -> R {
        f(&mut self.layers[id])
    }

    pub fn get_layer_mut(&mut self, id: LayerId) -> &mut Window {
        &mut self.layers[id]
    }

    pub fn new_layer(&mut self, window: Window) -> LayerId {
        self.layers.push(window);
        self.layers.len()-1
    }

    pub fn move_to(&mut self, id: LayerId, pos: Vec2<i32>) {
        self.layers[id].move_to(pos);
    }

    pub fn move_relative(&mut self, id: LayerId, pos_diff: Vec2<i32>) {
        self.layers[id].move_relative(pos_diff);
    }

    pub fn draw(&mut self) {
        for id in &self.layer_stack {
            self.layers[*id].draw_to(&mut self.buffer);
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
}
