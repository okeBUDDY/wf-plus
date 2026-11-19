//! The cooker's side of [`wf_art::slug`]: layers go straight into the pack
//! writer.

use crate::geom::Curve;
use wf_art::Writer;
pub use wf_art::slug::{BAND_MARGIN, Edges, LayerGeometry, MAX_BANDS, gpu_layer};

/// Appends `curves` (a filled layer) and their bands to `w`.
pub fn add_layer(w: &mut Writer, curves: &[Curve]) -> LayerGeometry {
    add_edges(w, Edges::Fill(curves))
}

/// Appends a layer of any kind to `w`.
pub fn add_edges(w: &mut Writer, edges: Edges) -> LayerGeometry {
    let mut t = wf_art::slug::Target { curves: &mut w.curves, bands: &mut w.bands, curve_base: 0, band_base: 0 };
    wf_art::slug::add_layer(&mut t, edges)
}
