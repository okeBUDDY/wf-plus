//! The game's 2D transform (hl_3d.h): a 3x3 matrix applied to row vectors,
//! x' = x * m[0][0] + y * m[1][0] + m[2][0] and
//! y' = x * m[0][1] + y * m[1][1] + m[2][1]. The helpers keep the original operation order so results
//! match the original's float behaviour as closely as f32 allows.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub m: [[f32; 3]; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Transform = Transform { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] };

    pub fn translation(x: f32, y: f32) -> Self {
        let mut t = Self::IDENTITY;
        t.translate(x, y);
        t
    }

    /// `ScaleTransform`.
    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.m[0][0] *= sx;
        self.m[1][0] *= sx;
        self.m[2][0] *= sx;
        self.m[0][1] *= sy;
        self.m[1][1] *= sy;
        self.m[2][1] *= sy;
    }

    /// `TranslateTransform`.
    pub fn translate(&mut self, x: f32, y: f32) {
        self.m[2][0] += x;
        self.m[2][1] += y;
    }

    /// `MultiplyTransform(out, a, b)`: out = b x a in the original's naming
    /// (row-vector convention: apply `a` after `b`... see 0x4023d0).
    pub fn multiply(a: &Transform, b: &Transform) -> Transform {
        let mut p = [[0.0f32; 3]; 3];
        for (r, row) in p.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = b.m[r][0] * a.m[0][c] + b.m[r][1] * a.m[1][c] + b.m[r][2] * a.m[2][c];
            }
        }
        Transform { m: p }
    }

    /// `RotateTransformZ`: rotates by `radians` (applied after the current
    /// transform's linear part, as the original multiplies Rotation x T).
    pub fn rotate_z(&mut self, radians: f32) {
        let mut r = Self::IDENTITY;
        r.m[0][1] = radians.sin();
        r.m[1][0] = -r.m[0][1];
        r.m[1][1] = radians.cos();
        r.m[0][0] = r.m[1][1];
        *self = Transform::multiply(&r, self);
    }

    /// `TransformPoint`.
    pub fn apply(&self, x: f32, y: f32) -> [f32; 2] {
        [
            self.m[0][0] * x + self.m[1][0] * y + self.m[2][0],
            self.m[0][1] * x + self.m[1][1] * y + self.m[2][1],
        ]
    }
}
