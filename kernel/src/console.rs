use crate::{graphics::{PixelColor, Graphics}, font::{write_ascii, write_string}};

const ROWS: usize = 35;
const COLS: usize = 100;
pub struct Console<'a> {
    graphics: &'a mut Graphics<'a>,
    fg_color: PixelColor,
    bg_color: PixelColor,
    buffer: [[u8;COLS];ROWS],
    cursor_row: usize,
    cursor_col: usize
}


impl<'a> Console<'a> {
    pub fn new(graphics: &'a mut Graphics<'a>, fg_color: PixelColor, bg_color: PixelColor) -> Self {
        Self { graphics, fg_color, bg_color, buffer: [[0;COLS];ROWS], cursor_row: 0, cursor_col: 0 }
    }

    fn scroll_up(&mut self) {
        for y in 0..16 * ROWS {
            for x in 0..8 * COLS {
                self.graphics.write_pixel((x as u32, y as u32).into(), self.bg_color);
            }
        }
        for row in 0..ROWS-1 {
            self.buffer[row] = self.buffer[row+1];
            write_string(self.graphics, 0, row as u32 * 16, &self.buffer[row], self.fg_color);
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
            } else {
                if self.cursor_col < COLS {
                    write_ascii(self.graphics, 8 * self.cursor_col as u32, 16 * self.cursor_row as u32, *c as char, self.fg_color);
                    self.buffer[self.cursor_row][self.cursor_col] = *c;
                    self.cursor_col += 1;
                }
            }
        }
    }
}
