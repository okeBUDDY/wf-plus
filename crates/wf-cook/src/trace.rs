//! Traces bitmap art into exact pixel outlines, for art that has no hand-made
//! vector version yet (the fonts). Pixels of one colour become one layer
//! whose outline is the boundary of those pixels, so the result renders the
//! bitmap exactly at 1:1 and stays sharp when scaled.

use crate::geom::{Curve, Vec2};

pub struct Bitmap<'a> {
    pub width: usize,
    pub height: usize,
    /// Straight RGBA8, row major.
    pub rgba: &'a [u8],
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

/// One traced colour: the straight colour and its outline in rect-local
/// pixel coordinates.
pub struct TracedLayer {
    pub color: [f32; 4],
    pub curves: Vec<Curve>,
}

/// Rounds alpha to `levels` steps (antialiased fonts have up to 256).
fn quantize(a: u8, levels: u32) -> u8 {
    let step = 255.0 / (levels - 1) as f32;
    ((a as f32 / step).round() * step).round().clamp(0.0, 255.0) as u8
}

pub fn trace(bitmap: &Bitmap, rect: &Rect, alpha_levels: u32) -> Vec<TracedLayer> {
    let px = |x: usize, y: usize| -> [u8; 4] {
        let i = ((rect.y + y) * bitmap.width + rect.x + x) * 4;
        let p = &bitmap.rgba[i..i + 4];
        let a = quantize(p[3], alpha_levels);
        if a == 0 { [0, 0, 0, 0] } else { [p[0], p[1], p[2], a] }
    };
    let mut classes: Vec<[u8; 4]> = Vec::new();
    for y in 0..rect.h {
        for x in 0..rect.w {
            let p = px(x, y);
            if p[3] != 0 && !classes.contains(&p) {
                classes.push(p);
            }
        }
    }
    // Draw the faintest first; the classes are disjoint, so the order only
    // matters for determinism.
    classes.sort_by_key(|c| (c[3], c[0], c[1], c[2]));

    classes
        .into_iter()
        .map(|class| {
            let inside = |x: isize, y: isize| -> bool {
                x >= 0 && y >= 0 && (x as usize) < rect.w && (y as usize) < rect.h && px(x as usize, y as usize) == class
            };
            let mut curves = Vec::new();
            // Edges run clockwise around every pixel (y down); edges shared by
            // two inside pixels cancel, and runs in one direction merge.
            for y in 0..=rect.h as isize {
                let mut run: Option<(isize, i32)> = None;
                for x in 0..=rect.w as isize {
                    let dir = if x < rect.w as isize {
                        match (inside(x, y - 1), inside(x, y)) {
                            (false, true) => 1,
                            (true, false) => -1,
                            _ => 0,
                        }
                    } else {
                        0
                    };
                    if let Some((start, d)) = run {
                        if d != dir {
                            let (a, b) = (Vec2::new(start as f32, y as f32), Vec2::new(x as f32, y as f32));
                            curves.push(if d > 0 { Curve::line(a, b) } else { Curve::line(b, a) });
                            run = None;
                        }
                    }
                    if dir != 0 && run.is_none() {
                        run = Some((x, dir));
                    }
                }
            }
            for x in 0..=rect.w as isize {
                let mut run: Option<(isize, i32)> = None;
                for y in 0..=rect.h as isize {
                    let dir = if y < rect.h as isize {
                        match (inside(x - 1, y), inside(x, y)) {
                            (true, false) => 1,
                            (false, true) => -1,
                            _ => 0,
                        }
                    } else {
                        0
                    };
                    if let Some((start, d)) = run {
                        if d != dir {
                            let (a, b) = (Vec2::new(x as f32, start as f32), Vec2::new(x as f32, y as f32));
                            curves.push(if d > 0 { Curve::line(a, b) } else { Curve::line(b, a) });
                            run = None;
                        }
                    }
                    if dir != 0 && run.is_none() {
                        run = Some((y, dir));
                    }
                }
            }
            TracedLayer { color: class.map(|c| c as f32 / 255.0), curves }
        })
        .collect()
}
