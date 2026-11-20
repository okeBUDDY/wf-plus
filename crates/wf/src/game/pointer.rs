//! Pointer targets on the menus (the port's): the rectangles the mouse
//! and a finger hit, in the units a screen draws in. Display only: the
//! menus are never recorded.

/// A hit rectangle: [x0, x1) by [y0, y1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    pub const fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Rect { x0, y0, x1, y1 }
    }

    /// Text of `chars` characters `w` wide at (`x`, `y`), `h` high, with a
    /// margin of `m` all round.
    pub fn text(x: f32, y: f32, chars: usize, w: f32, h: f32, m: f32) -> Self {
        Rect::new(x - m, y - m, x + chars as f32 * w + m, y + h + m)
    }

    pub fn contains(&self, p: [f32; 2]) -> bool {
        (self.x0..self.x1).contains(&p[0]) && (self.y0..self.y1).contains(&p[1])
    }
}

/// `p` in the units of a block drawn moved by `origin`.
pub fn local(p: [f32; 2], origin: [f32; 2]) -> [f32; 2] {
    [p[0] - origin[0], p[1] - origin[1]]
}
