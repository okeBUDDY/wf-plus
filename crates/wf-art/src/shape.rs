//! Untextured shapes (what the original drew with `HL3D::DrawPoints` after
//! `ClearTexture`) as Slug layers. Triangle lists, strips and fans become
//! filled triangles; line lists and strips become stroke rectangles one
//! screen pixel wide, like Direct3D 7's lines, whatever the transform.
//!
//! The cooker turns the game's constant point tables into sprites
//! (`shape/<name>`); the game builds the shapes whose points come from game
//! logic at run time with the same code.

use crate::geom::{Curve, Vec2};
use crate::slug::StrokeEdge;

/// The primitive of a shape (D3DPRIMITIVETYPE).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Primitive {
    TriangleStrip,
    TriangleFan,
    TriangleList,
    LineStrip,
    LineList,
}

impl Primitive {
    pub fn is_line(self) -> bool {
        matches!(self, Primitive::LineStrip | Primitive::LineList)
    }
}

/// A constant shape of the game, cooked into the art pack as
/// `shape/<name>`.
pub struct ShapeDef {
    pub name: &'static str,
    pub primitive: Primitive,
    pub points: &'static [[f32; 2]],
    /// The smallest scale (screen pixels per local unit) the game draws a
    /// line shape at; it bounds how far a 1-pixel stroke reaches in local
    /// units, so its curves land in the right bands.
    pub min_scale: f32,
}

/// The outline of a shape, ready for [`crate::slug::add_layer`].
pub enum Outline {
    Fill(Vec<Curve>),
    Stroke(Vec<StrokeEdge>),
}

/// The stroke margin (local units) for a shape drawn at `min_scale` (pixels
/// of the 640x480 screen per local unit) or more: twice the half-pixel
/// offset of a one-pixel line, for safety.
pub fn stroke_margin(min_scale: f32) -> f32 {
    1.0 / min_scale.max(1e-3)
}

/// The outline of `points` drawn as `primitive`.
pub fn outline(primitive: Primitive, points: &[[f32; 2]]) -> Outline {
    let p = |i: usize| Vec2::new(points[i][0], points[i][1]);
    let n = points.len();
    match primitive {
        Primitive::TriangleStrip | Primitive::TriangleFan | Primitive::TriangleList => {
            let tris: Vec<[usize; 3]> = match primitive {
                Primitive::TriangleStrip => (0..n.saturating_sub(2)).map(|i| [i, i + 1, i + 2]).collect(),
                Primitive::TriangleFan => (1..n.saturating_sub(1)).map(|i| [0, i, i + 1]).collect(),
                _ => (0..n / 3).map(|i| [3 * i, 3 * i + 1, 3 * i + 2]).collect(),
            };
            let mut curves = Vec::new();
            for [a, b, c] in tris {
                let (a, mut b, mut c) = (p(a), p(b), p(c));
                let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
                if area == 0.0 {
                    continue;
                }
                // One orientation for all, so that overlaps add up (and
                // saturate) and shared edges cancel.
                if area < 0.0 {
                    std::mem::swap(&mut b, &mut c);
                }
                curves.extend([Curve::line(a, b), Curve::line(b, c), Curve::line(c, a)]);
            }
            Outline::Fill(curves)
        }
        Primitive::LineStrip | Primitive::LineList => {
            let segments: Vec<(Vec2, Vec2)> = if primitive == Primitive::LineStrip {
                (0..n.saturating_sub(1)).map(|i| (p(i), p(i + 1))).collect()
            } else {
                (0..n / 2).map(|i| (p(2 * i), p(2 * i + 1))).collect()
            };
            let mut edges = Vec::new();
            for (a, b) in segments {
                let d = b - a;
                let len = (d.x * d.x + d.y * d.y).sqrt();
                if len == 0.0 {
                    continue;
                }
                // The rectangle a..b, one width across: every segment has the
                // same orientation, so joints overlap and saturate.
                let nrm = Vec2::new(-d.y / len, d.x / len);
                edges.extend([
                    StrokeEdge { a, b, na: nrm, nb: nrm },
                    StrokeEdge { a: b, b, na: nrm, nb: -nrm },
                    StrokeEdge { a: b, b: a, na: -nrm, nb: -nrm },
                    StrokeEdge { a, b: a, na: -nrm, nb: nrm },
                ]);
            }
            Outline::Stroke(edges)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_are_oriented_one_way() {
        let Outline::Fill(c) = outline(Primitive::TriangleStrip, &[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]])
        else {
            panic!()
        };
        assert_eq!(c.len(), 6);
        // Each triangle's signed area is positive.
        for t in c.chunks(3) {
            let (a, b, cc) = (t[0].p1, t[1].p1, t[2].p1);
            assert!((b.x - a.x) * (cc.y - a.y) - (b.y - a.y) * (cc.x - a.x) > 0.0);
        }
    }

    #[test]
    fn lines_become_closed_rectangles() {
        let Outline::Stroke(e) = outline(Primitive::LineList, &[[0.0, 0.0], [10.0, 0.0]]) else { panic!() };
        assert_eq!(e.len(), 4);
        // Corners at offset 1 close the loop.
        let corner = |p: Vec2, n: Vec2| p + n;
        for i in 0..4 {
            let j = (i + 1) % 4;
            assert_eq!(corner(e[i].b, e[i].nb), corner(e[j].a, e[j].na));
        }
    }
}
