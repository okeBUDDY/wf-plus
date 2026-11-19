//! The SVG `feTurbulence` lattice (SVG 1.1, 15.24), baked at cook time.
//!
//! The filter's noise comes from a seeded lattice that is built by a
//! sequential random generator, which a shader cannot rebuild per pixel. The
//! cooker runs the spec's `init` here and stores the lattice selector and the
//! first colour channel's gradients in the layer texture (see
//! [`NoiseTable`]); the shader then evaluates the spec's `noise2` and
//! `turbulence` on it. Only the red channel is kept: the art's colour matrix
//! reads red alone.

const BSIZE: usize = 0x100;
const BM: i64 = 0xff;
const PERLIN_N: f64 = 4096.0;
const RAND_M: i64 = 2147483647;
const RAND_A: i64 = 16807;
const RAND_Q: i64 = 127773;
const RAND_R: i64 = 2836;

fn setup_seed(mut seed: i64) -> i64 {
    if seed <= 0 {
        seed = -(seed % (RAND_M - 1)) + 1;
    }
    if seed > RAND_M - 1 {
        seed = RAND_M - 1;
    }
    seed
}

fn random(seed: i64) -> i64 {
    let mut r = RAND_A * (seed % RAND_Q) - RAND_R * (seed / RAND_Q);
    if r <= 0 {
        r += RAND_M;
    }
    r
}

/// The lattice for one seed. The spec's selector has `2 * BSIZE + 2`
/// entries, but its upper half repeats the lower one, so `BSIZE` are kept.
pub struct NoiseTable {
    pub selector: [u32; BSIZE],
    /// Channel 0's unit gradients.
    pub gradient: [[f64; 2]; BSIZE],
}

impl NoiseTable {
    /// The spec's `init(lSeed)`. feTurbulence rounds a fractional seed to the
    /// nearest integer first.
    pub fn new(seed: f32) -> Self {
        let mut seed = setup_seed(seed.round() as i64);
        let mut selector = [0u32; BSIZE];
        let mut gradient = [[0f64; 2]; BSIZE];
        for k in 0..4 {
            for i in 0..BSIZE {
                selector[i] = i as u32;
                let mut g = [0f64; 2];
                for c in &mut g {
                    seed = random(seed);
                    *c = ((seed % (2 * BSIZE as i64)) - BSIZE as i64) as f64 / BSIZE as f64;
                }
                let s = (g[0] * g[0] + g[1] * g[1]).sqrt();
                if k == 0 {
                    gradient[i] = [g[0] / s, g[1] / s];
                }
            }
        }
        let mut i = BSIZE - 1;
        while i > 0 {
            let k = selector[i];
            seed = random(seed);
            let j = (seed % BSIZE as i64) as usize;
            selector[i] = selector[j];
            selector[j] = k;
            i -= 1;
        }
        NoiseTable { selector, gradient }
    }

    fn sel(&self, i: i64) -> i64 {
        self.selector[(i & BM) as usize] as i64
    }

    /// The spec's `noise2` for channel 0, without stitching.
    pub fn noise2(&self, x: f64, y: f64) -> f64 {
        let t = x + PERLIN_N;
        let bx0 = (t as i64) & BM;
        let bx1 = (bx0 + 1) & BM;
        let rx0 = t - (t as i64) as f64;
        let rx1 = rx0 - 1.0;
        let t = y + PERLIN_N;
        let by0 = (t as i64) & BM;
        let by1 = (by0 + 1) & BM;
        let ry0 = t - (t as i64) as f64;
        let ry1 = ry0 - 1.0;
        let i = self.sel(bx0);
        let j = self.sel(bx1);
        // i + by0 < 2 * BSIZE, and the selector repeats with period BSIZE.
        let b00 = self.sel(i + by0) as usize;
        let b10 = self.sel(j + by0) as usize;
        let b01 = self.sel(i + by1) as usize;
        let b11 = self.sel(j + by1) as usize;
        let s = |t: f64| t * t * (3.0 - 2.0 * t);
        let lerp = |t: f64, a: f64, b: f64| a + t * (b - a);
        let sx = s(rx0);
        let sy = s(ry0);
        let g = &self.gradient;
        let u = rx0 * g[b00][0] + ry0 * g[b00][1];
        let v = rx1 * g[b10][0] + ry0 * g[b10][1];
        let a = lerp(sx, u, v);
        let u = rx0 * g[b01][0] + ry1 * g[b01][1];
        let v = rx1 * g[b11][0] + ry1 * g[b11][1];
        let b = lerp(sx, u, v);
        lerp(sy, a, b)
    }

    /// `type="fractalNoise"` at a user-space point, mapped to 0..1 as the
    /// filter's colour result is (before clamping).
    pub fn fractal(&self, x: f64, y: f64, frequency: f64, octaves: u32) -> f64 {
        let (mut vx, mut vy) = (x * frequency, y * frequency);
        let mut sum = 0.0;
        let mut ratio = 1.0;
        for _ in 0..octaves {
            sum += self.noise2(vx, vy) / ratio;
            vx *= 2.0;
            vy *= 2.0;
            ratio *= 2.0;
        }
        (sum + 1.0) / 2.0
    }

    /// The table as `BSIZE` layer-texture texels: (selector, gradient x bits,
    /// gradient y bits, 0).
    pub fn texels(&self) -> Vec<[u32; 4]> {
        (0..BSIZE)
            .map(|i| {
                let g = self.gradient[i];
                [self.selector[i], (g[0] as f32).to_bits(), (g[1] as f32).to_bits(), 0]
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generator_is_the_park_miller_minimal_standard() {
        // 10000 steps from seed 1 give 1043618065 (Park & Miller 1988).
        let mut s = 1;
        for _ in 0..10000 {
            s = random(s);
        }
        assert_eq!(s, 1043618065);
    }

    #[test]
    fn the_lattice_is_a_permutation_with_unit_gradients() {
        let t = NoiseTable::new(7.0);
        let mut seen = [false; BSIZE];
        for &s in &t.selector {
            assert!(!seen[s as usize]);
            seen[s as usize] = true;
        }
        for g in &t.gradient {
            assert!(((g[0] * g[0] + g[1] * g[1]) - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn noise_is_zero_on_lattice_points_and_fractal_stays_in_range() {
        let t = NoiseTable::new(7.0);
        assert_eq!(t.noise2(3.0, 5.0), 0.0);
        for y in 0..64 {
            for x in 0..64 {
                let n = t.fractal(x as f64, y as f64, 0.045, 3);
                assert!((-0.2..=1.2).contains(&n), "{n}");
            }
        }
    }
}
