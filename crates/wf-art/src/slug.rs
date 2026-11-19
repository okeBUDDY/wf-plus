//! Builds the Slug data of one layer: its curves go into the curve texture
//! and its horizontal and vertical bands into the band texture. Used by the
//! cooker for the art and by the game for the shapes whose points come from
//! game logic.
//!
//! A pixel in band b only looks at the curves of band b, so a curve is put
//! in every band its extent overlaps, widened by [`BAND_MARGIN`] so that the
//! antialiasing of a pixel next to a band edge still sees it.
//!
//! A **stroke** layer (see [`crate::paint::FLAG_STROKE`]) holds the outline of
//! lines of a constant width in screen pixels. Each of its edges is stored as
//! two centre points and two signed unit normals, `(a, b)` and `(na, nb)`;
//! the shader puts the corners at `a + na * w` and `b + nb * w`, where `w` is
//! half the line width in pixels converted to local units along that normal
//! under the instance's transform. Bands are built for the widest offset the
//! stroke can have, the layer's stroke margin.

use crate::geom::{Curve, Vec2};
use crate::{CURVE_TEX_WIDTH, GpuLayer};

/// How far (in local units) a curve may lie outside a band and still count
/// for it. One art pixel covers the half-pixel filter footprint down to a
/// draw scale of 0.5.
pub const BAND_MARGIN: f32 = 1.0;
pub const MAX_BANDS: usize = 32;

fn band_count(curves: usize, extent: f32) -> usize {
    let by_curves = (curves as f32).sqrt().ceil() as usize;
    let by_size = (extent / 2.0).ceil().max(1.0) as usize;
    by_curves.min(by_size).clamp(1, MAX_BANDS)
}

/// One edge of a stroke outline: from `a + na * w` to `b + nb * w`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeEdge {
    pub a: Vec2,
    pub b: Vec2,
    pub na: Vec2,
    pub nb: Vec2,
}

/// What a layer is made of.
pub enum Edges<'a> {
    /// Closed contours of quadratic curves, filled by nonzero winding.
    Fill(&'a [Curve]),
    /// Stroke outlines; `margin` is the largest offset (local units) the
    /// shader can give a corner.
    Stroke(&'a [StrokeEdge], f32),
}

/// Where a layer's texels go: the curve and band texels so far, and the
/// texel index of their first element (non-zero for a region appended after
/// other data).
pub struct Target<'a> {
    pub curves: &'a mut Vec<[f32; 4]>,
    pub bands: &'a mut Vec<[u32; 2]>,
    pub curve_base: u32,
    pub band_base: u32,
}

pub struct LayerGeometry {
    pub bbox: [f32; 4],
    pub band_base: u32,
    pub hbands: u32,
    pub vbands: u32,
    pub band_scale: [f32; 2],
}

/// One edge as stored: its two texels, the curve whose direction decides
/// which rays can cross it, and its extent.
type Item = ([f32; 4], [f32; 4], Curve, [f32; 4]);

/// Appends a layer's curves and bands to `t`.
pub fn add_layer(t: &mut Target, edges: Edges) -> LayerGeometry {
    let items: Vec<Item> = match edges {
        Edges::Fill(curves) => curves
            .iter()
            .map(|c| {
                (
                    [c.p1.x, c.p1.y, c.p2.x, c.p2.y],
                    [c.p3.x, c.p3.y, 0.0, 0.0],
                    *c,
                    [c.min_x(), c.min_y(), c.max_x(), c.max_y()],
                )
            })
            .collect(),
        Edges::Stroke(edges, margin) => edges
            .iter()
            .map(|e| {
                let (pa, pb) = (e.a + e.na * margin, e.b + e.nb * margin);
                let xs = [e.a.x, e.b.x, pa.x, pb.x];
                let ys = [e.a.y, e.b.y, pa.y, pb.y];
                let ext = [
                    xs.iter().copied().fold(f32::MAX, f32::min),
                    ys.iter().copied().fold(f32::MAX, f32::min),
                    xs.iter().copied().fold(f32::MIN, f32::max),
                    ys.iter().copied().fold(f32::MIN, f32::max),
                ];
                // The edge at the widest offset: its direction is the edge's
                // at any offset, which decides which rays can cross it.
                ([e.a.x, e.a.y, e.b.x, e.b.y], [e.na.x, e.na.y, e.nb.x, e.nb.y], Curve::line(pa, pb), ext)
            })
            .collect(),
    };
    let mut bbox = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for (.., e) in &items {
        bbox = [bbox[0].min(e[0]), bbox[1].min(e[1]), bbox[2].max(e[2]), bbox[3].max(e[3])];
    }
    if items.is_empty() {
        bbox = [0.0; 4];
    }
    // Curve texels. Two texels per curve; a curve must not straddle a row.
    let width_texels = CURVE_TEX_WIDTH as usize;
    let mut curve_index = Vec::with_capacity(items.len());
    for (t0, t1, ..) in &items {
        let at = t.curve_base as usize + t.curves.len();
        if width_texels - at % width_texels < 2 {
            t.curves.push([0.0; 4]);
        }
        curve_index.push(t.curve_base + t.curves.len() as u32);
        t.curves.push(*t0);
        t.curves.push(*t1);
    }

    let width = (bbox[2] - bbox[0]).max(1e-3);
    let height = (bbox[3] - bbox[1]).max(1e-3);
    let hbands = band_count(items.len(), height);
    let vbands = band_count(items.len(), width);
    let hscale = hbands as f32 / height;
    let vscale = vbands as f32 / width;

    // Horizontal bands: curves a horizontal ray can cross (not flat in y),
    // sorted by descending max x.
    let mut hlists: Vec<Vec<usize>> = vec![Vec::new(); hbands];
    let mut vlists: Vec<Vec<usize>> = vec![Vec::new(); vbands];
    for (i, (_, _, c, e)) in items.iter().enumerate() {
        if c.min_y() != c.max_y() {
            let b0 = (((e[1] - BAND_MARGIN - bbox[1]) * hscale).floor().max(0.0)) as usize;
            let b1 = (((e[3] + BAND_MARGIN - bbox[1]) * hscale).floor() as usize).min(hbands - 1);
            for list in &mut hlists[b0..=b1] {
                list.push(i);
            }
        }
        if c.min_x() != c.max_x() {
            let b0 = (((e[0] - BAND_MARGIN - bbox[0]) * vscale).floor().max(0.0)) as usize;
            let b1 = (((e[2] + BAND_MARGIN - bbox[0]) * vscale).floor() as usize).min(vbands - 1);
            for list in &mut vlists[b0..=b1] {
                list.push(i);
            }
        }
    }
    for list in &mut hlists {
        list.sort_by(|&a, &b| items[b].3[2].total_cmp(&items[a].3[2]));
    }
    for list in &mut vlists {
        list.sort_by(|&a, &b| items[b].3[3].total_cmp(&items[a].3[3]));
    }

    let band_base = t.band_base + t.bands.len() as u32;
    let mut next = band_base + (hbands + vbands) as u32;
    for list in hlists.iter().chain(vlists.iter()) {
        t.bands.push([list.len() as u32, next]);
        next += list.len() as u32;
    }
    for list in hlists.iter().chain(vlists.iter()) {
        for &i in list {
            t.bands.push([curve_index[i], 0]);
        }
    }

    LayerGeometry { bbox, band_base, hbands: hbands as u32, vbands: vbands as u32, band_scale: [vscale, hscale] }
}

pub fn gpu_layer(g: &LayerGeometry) -> GpuLayer {
    GpuLayer {
        bbox: g.bbox.map(f32::to_bits),
        band_base: g.band_base,
        hbands: g.hbands,
        vbands: g.vbands,
        band_scale: g.band_scale.map(f32::to_bits),
        ..Default::default()
    }
}
