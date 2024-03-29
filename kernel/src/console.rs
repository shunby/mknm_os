use core::iter::repeat_with;

use alloc::vec::Vec;
use lock_api::MutexGuard;
use x86_64::instructions::interrupts::without_interrupts;

use crate::{graphic::{font::{write_ascii, write_string}, frame_buffer::FrameBuffer, graphics::{PixelColor, Rect}, window::{LayerHandle, LayerId, Window}, with_layers}, memory_manager::{LazyInit, SpinMutex}, PixelWriter};

static CONSOLE: LazyInit<Console> = LazyInit::new();

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

/// コンソールとコンソールウィンドウを初期化
pub fn init_console(fg_color: (u8, u8, u8), bg_color: (u8, u8, u8)) {
    with_layers(|l| {
        let res = l.resolution();
        let win = Window::new(res.0 as usize, res.1 as usize);
        let hndl = l.new_layer(win);

        l.up_down(hndl.layer_id(), 0);
        CONSOLE.lock().init(Console::new(hndl, fg_color, bg_color));
    });
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {{
        $crate::console::_print(core::format_args!($($arg)*));
        $crate::print!("\n")

    }};
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        $crate::console::_print(core::format_args!($($arg)*));
    }};
}

pub fn _print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    CONSOLE.lock().write_fmt(args).unwrap();
}

impl Console {
    pub fn new(layer_handle: LayerHandle, fg_color: PixelColor, bg_color: PixelColor) -> Self {
        let (n_cols, n_rows) = {
            let window = layer_handle.window().read();
            (window.width() / CHAR_W, window.height() / CHAR_H)
        };
        let buffer: Vec<Vec<u8>> = repeat_with(||{vec![0u8;n_cols]}).take(n_rows).collect();

        {
            layer_handle.window().read().buffer().write_with(|back|{
                for y in 0..16 * n_rows {
                    for x in 0..8 * n_cols {
                        back.write((x as i32, y as i32).into(), bg_color);
                    }
                }
            });
        }

        Self { layer_handle, fg_color, bg_color, n_cols, n_rows, buffer, cursor_row: 0, cursor_col: 0 }
    }

    fn scroll_up(& mut self, window: &mut FrameBuffer) {
        window.move_rect((0,0).into(), Rect::from_points(0, 16, 8*self.n_cols as i32, 16*self.n_rows as i32));
        
        for y in 16*(self.n_rows-1)..16 * self.n_rows {
            for x in 0..8 * self.n_cols {
                window.write((x as i32, y as i32).into(), self.bg_color);
            }
        }

        for row in 0..self.n_rows-1 {
            self.buffer.swap(row, row+1);
        }
        self.buffer[self.n_rows-1].fill(0u8);
    }

    fn new_line(& mut self, window: &mut FrameBuffer) {
        self.cursor_col = 0;

        if self.cursor_row < self.n_rows - 1 { 
            self.cursor_row += 1;
        } else {
            self.scroll_up(window);
        }
    }

    pub fn put_string(&mut self, str: &[u8]) {
        let window = self.layer_handle.window().clone();
        let mut window_guard = window.read();
        
        window_guard.buffer().write_with(|back|{

            for c in str {
                if *c as char == '\n' {
                    self.new_line(back);
                } 
                write_ascii(back, 8 * self.cursor_col as u32, 16 * self.cursor_row as u32, *c as char, self.fg_color);
                self.buffer[self.cursor_row][self.cursor_col] = *c;
                self.cursor_col += 1;
                if self.cursor_col == self.n_cols {                
                    self.new_line(back);
                }        
            }
        });
        window_guard.buffer().flush();
    }
}

impl  core::fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.put_string(s.as_bytes());
        Ok(())
    }
}
