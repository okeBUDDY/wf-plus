//! Builds the Slug data of one layer: its curves go into the curve texture
//! and its horizontal and vertical bands into the band texture.
//!
//! A pixel in band b only looks at the curves of band b, so a curve is put
//! in every band its extent overlaps, widened by [`BAND_MARGIN`] so that the
//! antialiasing of a pixel next to a band edge still sees it.

use crate::geom::Curve;
use wf_art::{GpuLayer, Writer};

/// How far (in art pixels) a curve may lie outside a band and still count
/// for it. One art pixel covers the half-pixel filter footprint down to a
/// draw scale of 0.5.
pub const BAND_MARGIN: f32 = 1.0;
pub const MAX_BANDS: usize = 32;

fn band_count(curves: usize, extent: f32) -> usize {
    let by_curves = (curves as f32).sqrt().ceil() as usize;
    let by_size = (extent / 2.0).ceil().max(1.0) as usize;
    by_curves.min(by_size).clamp(1, MAX_BANDS)
}

pub struct LayerGeometry {
    pub bbox: [f32; 4],
    pub band_base: u32,
    pub hbands: u32,
    pub vbands: u32,
    pub band_scale: [f32; 2],
}

/// Appends `curves` and their bands to `w`.
pub fn add_layer(w: &mut Writer, curves: &[Curve]) -> LayerGeometry {
    let mut bbox = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for c in curves {
        bbox[0] = bbox[0].min(c.min_x());
        bbox[1] = bbox[1].min(c.min_y());
        bbox[2] = bbox[2].max(c.max_x());
        bbox[3] = bbox[3].max(c.max_y());
    }
    // Curve texels. Two texels per curve; a curve must not straddle a row.
    let mut curve_index = Vec::with_capacity(curves.len());
    for c in curves {
        let row_left = wf_art::CURVE_TEX_WIDTH as usize - w.curves.len() % wf_art::CURVE_TEX_WIDTH as usize;
        if row_left < 2 {
            w.curves.push([0.0; 4]);
        }
        curve_index.push(w.curves.len() as u32);
        w.curves.push([c.p1.x, c.p1.y, c.p2.x, c.p2.y]);
        w.curves.push([c.p3.x, c.p3.y, 0.0, 0.0]);
    }

    let width = (bbox[2] - bbox[0]).max(1e-3);
    let height = (bbox[3] - bbox[1]).max(1e-3);
    let hbands = band_count(curves.len(), height);
    let vbands = band_count(curves.len(), width);
    let hscale = hbands as f32 / height;
    let vscale = vbands as f32 / width;

    // Horizontal bands: curves a horizontal ray can cross (not flat in y),
    // sorted by descending max x.
    let mut hlists: Vec<Vec<usize>> = vec![Vec::new(); hbands];
    let mut vlists: Vec<Vec<usize>> = vec![Vec::new(); vbands];
    for (i, c) in curves.iter().enumerate() {
        if c.min_y() != c.max_y() {
            let b0 = (((c.min_y() - BAND_MARGIN - bbox[1]) * hscale).floor().max(0.0)) as usize;
            let b1 = (((c.max_y() + BAND_MARGIN - bbox[1]) * hscale).floor() as usize).min(hbands - 1);
            for list in &mut hlists[b0..=b1] {
                list.push(i);
            }
        }
        if c.min_x() != c.max_x() {
            let b0 = (((c.min_x() - BAND_MARGIN - bbox[0]) * vscale).floor().max(0.0)) as usize;
            let b1 = (((c.max_x() + BAND_MARGIN - bbox[0]) * vscale).floor() as usize).min(vbands - 1);
            for list in &mut vlists[b0..=b1] {
                list.push(i);
            }
        }
    }
    for list in &mut hlists {
        list.sort_by(|&a, &b| curves[b].max_x().total_cmp(&curves[a].max_x()));
    }
    for list in &mut vlists {
        list.sort_by(|&a, &b| curves[b].max_y().total_cmp(&curves[a].max_y()));
    }

    let band_base = w.bands.len() as u32;
    let mut next = band_base + (hbands + vbands) as u32;
    for list in hlists.iter().chain(vlists.iter()) {
        w.bands.push([list.len() as u32, next]);
        next += list.len() as u32;
    }
    for list in hlists.iter().chain(vlists.iter()) {
        for &i in list {
            w.bands.push([curve_index[i], 0]);
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
