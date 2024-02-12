use core::iter::repeat_with;

use alloc::vec::Vec;
use lock_api::MutexGuard;

use crate::{font::{write_ascii, write_string}, graphics::{PixelColor, Rect}, memory_manager::SingleMutex, window::{LayerHandle, LayerId, Window}, PixelWriter, LAYERS};

const CHAR_W: usize = 8;
const CHAR_H: usize = 16;
pub struct Console {
    layer_handle: LayerHandle,
    fg_color: PixelColor,
    bg_color: PixelColor,
    n_rows: usize,
    n_cols: usize,
    buffer: Vec<Vec<u8>>,
    cursor_row: usize,
    cursor_col: usize
}

impl Console {
    pub fn new(layer_handle: LayerHandle, fg_color: PixelColor, bg_color: PixelColor) -> Self {
        let (n_cols, n_rows) = {
            let window = layer_handle.window().lock();
            (window.width() / CHAR_W, window.height() / CHAR_H)
        };
        let buffer: Vec<Vec<u8>> = repeat_with(||{vec![0u8;n_cols]}).take(n_rows).collect();

        {
            let mut window = layer_handle.window().lock();
            for y in 0..16 * n_rows {
                for x in 0..8 * n_cols {
                    window.write((x as i32, y as i32).into(), bg_color);
                }
            }
        }

        Self { layer_handle, fg_color, bg_color, n_cols, n_rows, buffer, cursor_row: 0, cursor_col: 0 }
    }

    fn scroll_up(& mut self, window_lock: &mut MutexGuard<'_, SingleMutex, Window>) {
        window_lock.move_rect((0,0).into(), Rect::from_points(0, 16, 8*self.n_cols as i32, 16*self.n_rows as i32));

        for y in 16*(self.n_rows-1)..16 * self.n_rows {
            for x in 0..8 * self.n_cols {
                window_lock.write((x as i32, y as i32).into(), self.bg_color);
            }
        }
        for row in 0..self.n_rows-1 {
            self.buffer.swap(row, row+1);
        }
        self.buffer[self.n_rows-1].fill(0u8);
    }

    fn new_line(& mut self, window_lock: &mut MutexGuard<'_, SingleMutex, Window>) {
        self.cursor_col = 0;

        if self.cursor_row < self.n_rows - 1 { 
            self.cursor_row += 1;
        } else {
            self.scroll_up(window_lock);
        }
    }

    pub fn put_string(&mut self, str: &[u8]) {
        {let window = self.layer_handle.window().clone();
        let mut window_guard = window.lock();

        for c in str {
            if *c as char == '\n' {
                self.new_line(&mut window_guard);
            } 
            write_ascii(&mut *window_guard, 8 * self.cursor_col as u32, 16 * self.cursor_row as u32, *c as char, self.fg_color);
            self.buffer[self.cursor_row][self.cursor_col] = *c;
            self.cursor_col += 1;
            if self.cursor_col == self.n_cols {                
                self.new_line(&mut window_guard);
            }
            
        }}
        let mut layers = LAYERS.lock();
        layers.draw();
    }
    

}

impl  core::fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.put_string(s.as_bytes());
        Ok(())
    }
}
