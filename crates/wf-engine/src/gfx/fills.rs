//! Fill shapes built at run time (the aim cone, the radar's weapon chart)
//! drawn without Slug's bands: one instance per triangle, rasterized as
//! the triangle grown by a pixel and a quarter, whose fragment shader
//! (`fs_fill` in slug.wgsl) sums the crossings of all the shape's
//! triangles' edges as Slug does, so the coverage is Slug's. A pixel then
//! costs a few edge tests near the shape, not a band of curves anywhere in
//! its box (the aim cone is a long thin wedge, mostly off screen).
//!
//! Where the grown triangles overlap, the lowest-numbered triangle whose
//! triangle grown by three quarters of a pixel holds the pixel draws it
//! (every pixel with coverage is that close to some triangle, and the
//! rasterized triangle is a half pixel bigger, so exactly one draws it).

use super::Primitive;
use wf_art::shape::{Outline, outline};

/// The triangles of a fill shape as Slug's outline orients them (zero-area
/// ones dropped); empty for a line shape.
pub fn triangles(primitive: Primitive, points: &[[f32; 2]]) -> Vec<[[f32; 2]; 3]> {
    if primitive.is_line() {
        return Vec::new();
    }
    match outline(primitive, points) {
        Outline::Fill(curves) => curves.chunks_exact(3).map(|c| [c[0].p1, c[1].p1, c[2].p1].map(|p| [p.x, p.y])).collect(),
        Outline::Stroke(_) => Vec::new(),
    }
}

/// For each triangle, the edges (bit 0: a to b, 1: b to c, 2: c to a)
/// another triangle has the other way round: their crossings cancel.
pub fn shared_edges(tris: &[[[f32; 2]; 3]]) -> Vec<u32> {
    let bits = |p: [f32; 2]| (p[0].to_bits(), p[1].to_bits());
    let edges = |t: &[[f32; 2]; 3]| [(bits(t[0]), bits(t[1])), (bits(t[1]), bits(t[2])), (bits(t[2]), bits(t[0]))];
    let all: Vec<_> = tris.iter().map(edges).collect();
    all.iter()
        .enumerate()
        .map(|(i, es)| {
            let mut mask = 0;
            for (e, &(p, q)) in es.iter().enumerate() {
                if all.iter().enumerate().any(|(j, other)| j != i && other.contains(&(q, p))) {
                    mask |= 1 << e;
                }
            }
            mask
        })
        .collect()
}

/// The device pixels of triangle `t` under `m` inside the window, roughly:
/// its area times the share of its box inside the window, plus a margin
/// along its perimeter.
pub fn area(t: &[[f32; 2]; 3], m: &[[f32; 3]; 3], layout: &super::Layout, size: (u32, u32)) -> f64 {
    let k = layout.scale;
    let dev = |p: [f32; 2]| {
        [
            (p[0] * m[0][0] + p[1] * m[1][0] + m[2][0] - layout.visible[0]) * k,
            (p[0] * m[0][1] + p[1] * m[1][1] + m[2][1] - layout.visible[1]) * k,
        ]
    };
    let [a, b, c] = t.map(dev);
    let full = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() as f64 * 0.5;
    let x0 = a[0].min(b[0]).min(c[0]);
    let x1 = a[0].max(b[0]).max(c[0]);
    let y0 = a[1].min(b[1]).min(c[1]);
    let y1 = a[1].max(b[1]).max(c[1]);
    let boxed = ((x1 - x0) * (y1 - y0)).max(1e-6) as f64;
    let inside = ((x1.min(size.0 as f32) - x0.max(0.0)).max(0.0) * (y1.min(size.1 as f32) - y0.max(0.0)).max(0.0)) as f64;
    let len = |p: [f32; 2], q: [f32; 2]| ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt() as f64;
    (full + 1.25 * (len(a, b) + len(b, c) + len(c, a))) * (inside / boxed).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangles_are_oriented_and_skip_degenerate_ones() {
        let t = triangles(Primitive::TriangleFan, &[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [0.0, 1.0], [-1.0, 0.0]]);
        assert_eq!(t.len(), 2);
        for [a, b, c] in t {
            assert!((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 0.0);
        }
        let fan = triangles(Primitive::TriangleFan, &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        assert_eq!(shared_edges(&fan), vec![4, 1]);
        assert!(triangles(Primitive::LineStrip, &[[0.0, 0.0], [1.0, 0.0]]).is_empty());
    }
}
