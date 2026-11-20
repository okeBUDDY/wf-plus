//! Line shapes (line lists and strips, one pixel of the 640x480 screen
//! wide) drawn without Slug's bands: one thin quad per segment, whose
//! fragment shader computes the coverage Slug would give the segment's
//! stroke rectangle analytically (`fs_line` in slug.wgsl). A line's pixels
//! are then those near it, not its shape's whole box, and each costs a
//! few edge tests, not a band of curves (the radar's hexagon on the
//! warning screen, the aim cone's sides, the grid).
//!
//! Slug sums the crossings of all the rectangles of a shape, so where two
//! segments overlap (a joint) the coverage saturates instead of blending
//! twice. Each segment instance knows the neighbour sharing each of its
//! endpoints: the pixel's coverage sums the three rectangles, and only the
//! lowest-numbered segment covering the pixel draws it.

use super::Primitive;

/// One segment and its neighbours: the other endpoint of the segment that
/// shares `a` (and of the one that shares `b`), if any.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub a: [f32; 2],
    pub b: [f32; 2],
    pub nbr_a: Option<[f32; 2]>,
    pub nbr_b: Option<[f32; 2]>,
    /// The neighbour comes first in the shape (it draws the pixels both
    /// cover).
    pub nbr_a_first: bool,
    pub nbr_b_first: bool,
}

/// Flag bits of a line instance.
pub const HAS_NBR_A: u32 = 1;
pub const HAS_NBR_B: u32 = 2;
pub const NBR_A_FIRST: u32 = 4;
pub const NBR_B_FIRST: u32 = 8;

impl Segment {
    pub fn flags(&self) -> u32 {
        (self.nbr_a.is_some() as u32 * HAS_NBR_A)
            | (self.nbr_b.is_some() as u32 * HAS_NBR_B)
            | (self.nbr_a_first as u32 * NBR_A_FIRST)
            | (self.nbr_b_first as u32 * NBR_B_FIRST)
    }
}

/// The segments of a line shape (empty for a fill), in Slug's order;
/// zero-length segments are dropped, as the cooker drops them.
pub fn segments(primitive: Primitive, points: &[[f32; 2]]) -> Vec<Segment> {
    let n = points.len();
    let pairs: Vec<([f32; 2], [f32; 2])> = match primitive {
        Primitive::LineStrip => (0..n.saturating_sub(1)).map(|i| (points[i], points[i + 1])).collect(),
        Primitive::LineList => (0..n / 2).map(|i| (points[2 * i], points[2 * i + 1])).collect(),
        _ => return Vec::new(),
    };
    let pairs: Vec<_> = pairs.into_iter().filter(|(a, b)| a != b).collect();
    let same = |p: [f32; 2], q: [f32; 2]| p[0].to_bits() == q[0].to_bits() && p[1].to_bits() == q[1].to_bits();
    // The lowest-numbered other segment with an endpoint at `p`: its index
    // and its other endpoint.
    let neighbour = |i: usize, p: [f32; 2]| {
        pairs.iter().enumerate().find_map(|(j, &(a, b))| {
            if j == i {
                None
            } else if same(a, p) {
                Some((j, b))
            } else if same(b, p) {
                Some((j, a))
            } else {
                None
            }
        })
    };
    pairs
        .iter()
        .enumerate()
        .map(|(i, &(a, b))| {
            let na = neighbour(i, a);
            let nb = neighbour(i, b);
            Segment {
                a,
                b,
                nbr_a: na.map(|n| n.1),
                nbr_b: nb.map(|n| n.1),
                nbr_a_first: na.is_some_and(|n| n.0 < i),
                nbr_b_first: nb.is_some_and(|n| n.0 < i),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_strip_links_around() {
        let hex = [[0.0, -2.0], [1.7, -1.0], [1.7, 1.0], [0.0, 2.0], [-1.7, 1.0], [-1.7, -1.0], [0.0, -2.0]];
        let s = segments(Primitive::LineStrip, &hex);
        assert_eq!(s.len(), 6);
        // The first segment's start is the last one's end.
        assert_eq!(s[0].nbr_a, Some([-1.7, -1.0]));
        assert!(!s[0].nbr_a_first && !s[0].nbr_b_first);
        assert_eq!(s[5].nbr_b, Some([1.7, -1.0]));
        assert!(s[5].nbr_a_first && s[5].nbr_b_first);
        assert_eq!(s[2].nbr_a, Some([1.7, -1.0]));
        assert_eq!(s[2].nbr_b, Some([-1.7, 1.0]));
    }

    #[test]
    fn a_list_links_shared_points_only() {
        let s = segments(Primitive::LineList, &[[0.0, 0.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [5.0, 5.0], [6.0, 5.0]]);
        assert_eq!(s.len(), 3);
        assert_eq!(s[1].nbr_a, Some([1.0, 0.0]));
        assert!(s[1].nbr_a_first);
        assert_eq!((s[2].nbr_a, s[2].nbr_b), (None, None));
        assert!(segments(Primitive::TriangleFan, &[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]).is_empty());
        assert!(segments(Primitive::LineStrip, &[[1.0, 1.0], [1.0, 1.0]]).is_empty());
    }
}
