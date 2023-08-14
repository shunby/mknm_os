use crate::{graphics::{PixelColor, Rect}, font::{write_ascii, write_string}, PixelWriter, LAYERS, window::LayerId};

const ROWS: usize = 30;
const COLS: usize = 80;
pub struct Console {
    layer_id: LayerId,
    fg_color: PixelColor,
    bg_color: PixelColor,
    buffer: [[u8;COLS];ROWS],
    cursor_row: usize,
    cursor_col: usize
}

impl Console {
    pub fn new(layer_id: LayerId, fg_color: PixelColor, bg_color: PixelColor) -> Self {
        Self { layer_id, fg_color, bg_color, buffer: [[0;COLS];ROWS], cursor_row: 0, cursor_col: 0 }
    }

    fn scroll_up(&mut self) {
        let mut layers = LAYERS.lock();
        let window = layers.get_layer_mut(self.layer_id);

        window.move_rect((0,0).into(), Rect::from_points(0, 16, 8*COLS as i32, 16*ROWS as i32));


        for y in 16*(ROWS-1)..16 * ROWS {
            for x in 0..8 * COLS {
                window.write((x as i32, y as i32).into(), self.bg_color);
            }
        }
        for row in 0..ROWS-1 {
            self.buffer[row] = self.buffer[row+1];
            write_string(window, 0, row as u32 * 16, &self.buffer[row], self.fg_color);
        }
        self.buffer[ROWS-1] = [0u8; COLS];
    }

    fn new_line(&mut self) {
        self.cursor_col = 0;

        if self.cursor_row < ROWS - 1 { 
            self.cursor_row += 1;
        } else {
            self.scroll_up();       
        }
    }

    pub fn put_string(&mut self, str: &[u8]) {
        for c in str {
            if *c as char == '\n' {
                self.new_line();
            } else if self.cursor_col < COLS {
                write_ascii(LAYERS.lock().get_layer_mut(self.layer_id), 8 * self.cursor_col as u32, 16 * self.cursor_row as u32, *c as char, self.fg_color);
                self.buffer[self.cursor_row][self.cursor_col] = *c;
                self.cursor_col += 1;
                
            }
        }
    }
}

impl  core::fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.put_string(s.as_bytes());
        Ok(())
    }
}
