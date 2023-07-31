use crate::{graphics::{PixelColor, Vec2}, GRAPHICS};


const MOUSE_CURSOR_DIMENSION: (usize, usize) = (15, 24);
const MOUSE_CURSOR_SHAPE: [&str; MOUSE_CURSOR_DIMENSION.1] = [
    "@              ",
    "@@             ",
    "@.@            ",
    "@..@           ",
    "@...@          ",
    "@....@         ",
    "@.....@        ",
    "@......@       ",
    "@.......@      ",
    "@........@     ",
    "@.........@    ",
    "@..........@   ",
    "@...........@  ",
    "@............@ ",
    "@......@@@@@@@@",
    "@......@       ",
    "@....@@.@      ",
    "@...@ @.@      ",
    "@..@   @.@     ",
    "@.@    @.@     ",
    "@@      @.@    ",
    "@       @.@    ",
    "@        @.@   ",
    "         @@@   ",
];

pub struct MouseCursor {
    erase_color: PixelColor,
    position: Vec2<i32>
}

impl MouseCursor {
    pub fn new(erase_color: PixelColor, initial_pos: Vec2<i32>) -> Self { 
        let mut new = Self {
            erase_color, position: initial_pos
        };
        new.draw_cursor();
        new
    }

    fn erase_cursor(&mut self) {
        let mut graphics = GRAPHICS.lock();
        for dy in 0..MOUSE_CURSOR_DIMENSION.1 {
            for dx in 0..MOUSE_CURSOR_DIMENSION.0 {
                let pos = &self.position + &Vec2::new(dx as i32, dy as i32);
                match MOUSE_CURSOR_SHAPE[dy].as_bytes()[dx] as char {
                    '.' | '@' => graphics.write_pixel((pos.x as u32, pos.y as u32).into(), self.erase_color),
                    _ => ()
                }
            }
        }

    }

    pub fn move_relative(&mut self, displacement: Vec2<i32>) {
        self.erase_cursor();
        self.position = &self.position + &displacement;
        self.position.x = self.position.x.max(0);
        self.position.y = self.position.y.max(0);
        self.draw_cursor();
    }

    fn draw_cursor(&mut self) {
        let mut graphics = GRAPHICS.lock();
        for dy in 0..MOUSE_CURSOR_DIMENSION.1 {
            for dx in 0..MOUSE_CURSOR_DIMENSION.0 {
                let pos = &self.position + &Vec2::new(dx as i32, dy as i32);
                match MOUSE_CURSOR_SHAPE[dy].as_bytes()[dx] as char {
                    '.' => graphics.write_pixel((pos.x as u32, pos.y as u32).into(), (255,255,255)),
                    '@' => graphics.write_pixel((pos.x as u32, pos.y as u32).into(), (0,0,0)),
                    _ => ()
                }
            }
        }

    }
}