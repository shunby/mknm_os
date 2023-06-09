use crate::graphics::{PixelColor, Vec2, Graphics};


const MOUSE_CURSOR_DIMENSION: (usize, usize) = (15, 24);
const MOUSE_CURSOR_SHAPE: [&'static str; MOUSE_CURSOR_DIMENSION.1] = [
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

pub struct MouseCursor<'a> {
    graphics: &'a mut Graphics<'a>,
    erase_color: PixelColor,
    position: Vec2<i32>
}

impl<'a> MouseCursor<'a> {
    pub fn new(graphics: &'a mut Graphics<'a>, erase_color: PixelColor, initial_pos: Vec2<i32>) -> Self { 
        let mut new = Self {
            graphics, erase_color, position: initial_pos
        };
        new.draw_cursor();
        new
    }

    fn erase_cursor(&mut self) {
        for dy in 0..MOUSE_CURSOR_DIMENSION.1 {
            for dx in 0..MOUSE_CURSOR_DIMENSION.0 {
                let pos = &self.position + &Vec2::new(dx as i32, dy as i32);
                match MOUSE_CURSOR_SHAPE[dy].as_bytes()[dx] as char {
                    '.' | '@' => self.graphics.write_pixel((pos.x as u32, pos.y as u32).into(), self.erase_color),
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
        for dy in 0..MOUSE_CURSOR_DIMENSION.1 {
            for dx in 0..MOUSE_CURSOR_DIMENSION.0 {
                let pos = &self.position + &Vec2::new(dx as i32, dy as i32);
                match MOUSE_CURSOR_SHAPE[dy].as_bytes()[dx] as char {
                    '.' => self.graphics.write_pixel((pos.x as u32, pos.y as u32).into(), (255,255,255)),
                    '@' => self.graphics.write_pixel((pos.x as u32, pos.y as u32).into(), (0,0,0)),
                    _ => ()
                }
            }
        }

    }
}