//! The game's maths helpers: the 1024-step sine table of HL3D
//! (CreateBuffers 0x4021e0), the C runtime's rand() and the float-to-int
//! conversions of the original.
//!
//! The original ran Direct3D 7 without DDSCL_FPUPRESERVE, so the x87 unit
//! worked at 24-bit (single) precision: plain f32 arithmetic here follows
//! it. Transcendentals are computed in f64 and rounded, as the CRT did.

use std::sync::OnceLock;

/// sin of `i` 1024ths of a turn for i in 0..0x500 (the extra quarter
/// turn gives the cosine as `SIN[i + 0x100]`).
// The original writes 3.14159265f, which is f32 PI.
#[allow(clippy::approx_constant)]
pub fn sin_table() -> &'static [f32; 0x500] {
    static TABLE: OnceLock<[f32; 0x500]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let step = 2.0f32 * 3.141_592_65_f32 / 1024.0f32;
        let mut t = [0.0f32; 0x500];
        for (i, v) in t.iter_mut().enumerate() {
            *v = ((i as f32 * step) as f64).sin() as f32;
        }
        t
    })
}

/// `g_sin_table[angle & 0x3ff]`.
#[inline]
pub fn sin(angle: i32) -> f32 {
    sin_table()[(angle & 0x3ff) as usize]
}

/// `g_sin_table[(angle + 0x100) & 0x3ff]`.
#[inline]
pub fn cos(angle: i32) -> f32 {
    sin_table()[(angle.wrapping_add(0x100) & 0x3ff) as usize]
}

/// `static_cast<int>(f)`: truncation (the CRT's _ftol); out of range
/// gives 0x80000000 like the x87 integer indefinite.
#[inline]
pub fn ftol(f: f32) -> i32 {
    if f.is_nan() || f >= 2147483648.0 || f < -2147483648.0 { i32::MIN } else { f as i32 }
}

/// `static_cast<short>(f)`.
#[inline]
pub fn ftos(f: f32) -> i16 {
    ftol(f) as i16
}

/// atan2 in 1024ths of a turn the way Actor::AngleTo computes it:
/// `0x100 - (int)(atan2(dy, dx) * -162.97466f)`.
#[inline]
pub fn angle_of(dx: f32, dy: f32) -> u16 {
    let a = (dy as f64).atan2(dx as f64) as f32;
    0x100i32.wrapping_sub(ftol(a * -162.974_66_f32)) as u16
}

/// The C runtime's rand()/srand() (MSVC libcmt): a 32-bit LCG returning
/// bits 16..30. Bit-exact, so replays of the original play back.
#[derive(Clone, Debug)]
pub struct Rand {
    pub seed: u32,
}

impl Default for Rand {
    fn default() -> Self {
        Rand { seed: 1 }
    }
}

impl Rand {
    pub fn srand(&mut self, seed: u32) {
        self.seed = seed;
    }

    /// rand(): 0..=0x7fff.
    pub fn rand(&mut self) -> i32 {
        self.seed = self.seed.wrapping_mul(214_013).wrapping_add(2_531_011);
        ((self.seed >> 16) & 0x7fff) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msvc_rand_sequence() {
        // srand(1) sequence of the Microsoft C runtime.
        let mut r = Rand::default();
        let got: Vec<i32> = (0..5).map(|_| r.rand()).collect();
        assert_eq!(got, [41, 18467, 6334, 26500, 19169]);
    }

    #[test]
    fn sine_table_quadrants() {
        assert_eq!(sin(0), 0.0);
        assert!((sin(0x100) - 1.0).abs() < 1e-6);
        assert!((cos(0) - 1.0).abs() < 1e-6);
        assert_eq!(angle_of(1.0, 0.0), 0x100);
        assert_eq!(angle_of(0.0, -1.0), 0);
    }
}
