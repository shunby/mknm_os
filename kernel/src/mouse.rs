use crate::graphic::graphics::{Vec2, PixelWriter};


const MOUSE_CURSOR_DIMENSION: (usize, usize) = (15, 24);
pub const MOUSE_CURSOR_SHAPE: [&str; MOUSE_CURSOR_DIMENSION.1] = [
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

pub fn draw_cursor(writer: &mut impl PixelWriter) {
    for dy in 0..MOUSE_CURSOR_DIMENSION.1 {
        let row = MOUSE_CURSOR_SHAPE[dy].as_bytes();
        for dx in 0..MOUSE_CURSOR_DIMENSION.0 {
            let pos = Vec2::new(dx as i32, dy as i32);
            match row[dx] as char {
                '.' => writer.write(pos, (255,255,255)),
                '@' => writer.write(pos, (0,0,0)),
                _ => writer.write(pos, (1,1,1))
            }
        }
    }
}
