//! 2D points and quadratic curves, shared by the cooker and the game.

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Vec2 { x, y }
    }
}

impl std::ops::Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl std::ops::Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, k: f32) -> Vec2 {
        Vec2::new(self.x * k, self.y * k)
    }
}

impl std::ops::Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

/// A quadratic Bézier from `p1` through control `p2` to `p3`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub p1: Vec2,
    pub p2: Vec2,
    pub p3: Vec2,
}

impl Curve {
    /// A straight segment, with its control point exactly at the midpoint so
    /// that the shader's quadratic term is exactly zero.
    pub fn line(a: Vec2, b: Vec2) -> Self {
        Curve { p1: a, p2: Vec2::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5), p3: b }
    }

    pub fn offset(&self, by: Vec2) -> Self {
        Curve { p1: self.p1 - by, p2: self.p2 - by, p3: self.p3 - by }
    }

    pub fn min_x(&self) -> f32 {
        self.p1.x.min(self.p2.x).min(self.p3.x)
    }
    pub fn max_x(&self) -> f32 {
        self.p1.x.max(self.p2.x).max(self.p3.x)
    }
    pub fn min_y(&self) -> f32 {
        self.p1.y.min(self.p2.y).min(self.p3.y)
    }
    pub fn max_y(&self) -> f32 {
        self.p1.y.max(self.p2.y).max(self.p3.y)
    }
}
