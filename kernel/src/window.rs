use core::iter::repeat_with;

use alloc::{vec::Vec, boxed::Box};

use crate::graphics::{PixelColor, Vec2, PixelWriter};

pub struct Window {
    pos: Vec2<i32>,
    width: usize,
    height: usize,
    data: Vec<Vec<PixelColor>>,
    transparant_color: Option<PixelColor>,
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

    pub fn draw_to(&self, writer: &mut Box<dyn PixelWriter>) {
        match self.transparant_color {
            None => {
                for (y, col) in self.data.iter().enumerate() {
                    for (x, pixel) in col.iter().enumerate() {
                        writer.write(&self.pos + &(x as i32, y as i32).into(), *pixel);
                    }
                }
            }
            Some(tc) => {
                for (y, col) in self.data.iter().enumerate() {
                    for (x, pixel) in col.iter().enumerate() {
                        if *pixel != tc {
                            writer.write(&self.pos + &(x as i32, y as i32).into(), *pixel);
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
}

impl PixelWriter for Window {
    fn write(&mut self, pos: Vec2<i32>, c: PixelColor) {
        if self.is_inside(pos) {
            *self.get_mut_at(pos) = c;
        }
    }
}

pub type LayerId = usize;

pub struct LayeredWindowManager {
    layers: Vec<Window>,
    layer_stack: Vec<LayerId>,
    writer: Box<dyn PixelWriter>,
}

impl LayeredWindowManager {
    pub fn new(writer: Box<dyn PixelWriter>) -> Self {
        Self {
            layers: Vec::new(),
            layer_stack: Vec::new(),
            writer
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
            self.layers[*id].draw_to(&mut self.writer);
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
