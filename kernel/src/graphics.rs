use core::ops::Add;

pub type PixelColor = (u8,u8,u8);

pub trait PixelWriter {
    fn write(&mut self, pos: Vec2<i32>, color: PixelColor);

    fn fill_rect(&mut self, pos: Vec2<i32>, size: Vec2<u32>, c: PixelColor) {
        for x in pos.x..pos.x + size.x as i32 {
            for y in pos.y..pos.y + size.y as i32 {
                self.write(&pos + &Vec2::new(x,y), c);
            }
        }
    }

    fn draw_bitpattern(&mut self, pos: Vec2<i32>, pattern: &[u64], c: PixelColor, scale: u32) {
        for dy in 0..pattern.len() {
            for dx in 0usize..64 {
                if (pattern[dy] >> (63-dx)) & 1 == 1 {
                    self.fill_rect(
                        &pos + &Vec2::new((scale*dx as u32) as i32, (scale * dy as u32) as i32),
                        Vec2::new(scale, scale), 
                        c
                    );
                }
            }
        }
    }
}


#[derive(Debug, Clone, Copy)]
pub struct Vec2<T>{
    pub x: T,
    pub y: T
}

impl<T> Vec2<T> {
    pub fn new(x: T, y: T) -> Self {
        Self {x,y}
    }
}

impl<T> Add<&Vec2<T>> for &Vec2<T> where for<'a, 'b> &'a T: Add<&'b T, Output = T>{
    type Output = Vec2<T>;
    fn add(self, rhs: &Vec2<T>) -> Self::Output {
        Vec2 {
            x: (&self.x) + (&rhs.x),
            y: (&self.y) + (&rhs.y) 
        }
    }
}


impl<T> Add<Vec2<T>> for Vec2<T> where T: Add<T, Output = T>{
    type Output = Vec2<T>;
    fn add(self, rhs: Vec2<T>) -> Self::Output {
        Vec2 {
            x: (self.x) + (rhs.x),
            y: (self.y) + (rhs.y) 
        }
    }
}

impl<T> From<(T,T)> for Vec2<T> {
    fn from(value: (T,T)) -> Self {
        Vec2 { x: value.0, y: value.1 }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Rect{
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32
}

impl Rect {
    pub fn from_points(x1: i32, y1: i32, x2: i32, y2: i32) -> Self {
        Self {x1, y1, x2, y2}
    }

    pub fn from_wh(x1: i32, y1: i32, w: i32, h: i32) -> Self {
        Self {x1, y1, x2: x1+w, y2: y1+h}
    }

    pub fn to_origin(&self) -> Self {
        Self {
            x1: 0,
            y1: 0,
            x2: self.x2 - self.x1,
            y2: self.y2 - self.y1
        }
    }

    pub fn move_relative(&self, dx: i32, dy: i32) -> Self {
        Self {
            x1: self.x1 + dx,
            x2: self.x2 + dx,
            y1: self.y1 + dy,
            y2: self.y2 + dy,
        }
    }

    pub fn contained_by(&self, other: &Self) -> bool {
        other.x1 <= self.x1 && other.y1 <= self.y1 && self.x2 <= other.x2 && self.y2 <= other.y2
    }

    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let x1 = self.x1.max(other.x1);
        let x2 = self.x2.min(other.x2);
        if x1 >= x2 {return None;}

        let y1 = self.y1.max(other.y1);
        let y2 = self.y2.min(other.y2);
        if y1 >= y2 {return None;}

        Some(Self {
            x1, x2, y1, y2
        })
    }
}
