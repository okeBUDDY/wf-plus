//! Cooks the art into the GPU and CPU halves at build time (see wf-art).
//!
//! Inputs: `WF_ART_DIR` (the vector art, one directory per sheet) and
//! `WF_FONT_DIR` (font0.png .. font2.png). Both default to the project's
//! shared folder next to this repository. `WF_BITMAP_DIR` holds the cut
//! original sprites for art that is still drawn as a bitmap. Vector glyphs
//! under `WF_GLYPH_DIR` (default `<art dir>/fonts`, file names from
//! `WF_GLYPH_PATTERNS`, see `wf_cook::GLYPH_PATTERNS`) replace the traced
//! glyphs of the font sheets one by one.

use std::path::PathBuf;

#[allow(dead_code)]
mod shape_table {
    include!("src/shape_table.rs");
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let game = manifest.join("../../../software/warning-forever/1.08");
    let dir = |var: &str, default: PathBuf| {
        println!("cargo:rerun-if-env-changed={var}");
        std::env::var_os(var).map(PathBuf::from).unwrap_or(default)
    };
    let art_dir = dir("WF_ART_DIR", game.join("sprites/vector"));
    let mut config = wf_cook::Config::new(art_dir.clone(), dir("WF_FONT_DIR", game.join("data")), dir("WF_BITMAP_DIR", game.join("sprites/sheets")));
    config.glyph_dir = dir("WF_GLYPH_DIR", art_dir.join("fonts"));
    config.glyph_csv = dir("WF_GLYPH_CSV", art_dir.join("../fonts/glyphs.csv"));
    println!("cargo:rerun-if-env-changed=WF_GLYPH_PATTERNS");
    if let Ok(p) = std::env::var("WF_GLYPH_PATTERNS") {
        // Separated by ';', e.g. "{font}/{font}_{index}.svg;{sheet}_{x}_{y}.svg".
        config.glyph_patterns = p.split(';').filter(|s| !s.is_empty()).map(str::to_string).collect();
    }
    config.shapes = shape_table::SHAPES;
    println!("cargo:rerun-if-changed=src/shape_table.rs");
    let (blob, report) = wf_cook::cook(&config).unwrap_or_else(|e| panic!("cooking the art failed: {e}"));
    // New vector glyphs appear as new files: watch the directories too.
    for w in &report.warnings {
        println!("cargo:warning=art: {w}");
    }
    for d in &report.dirs {
        if d.exists() {
            println!("cargo:rerun-if-changed={}", d.display());
        }
    }
    for input in &report.inputs {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("art.wfa");
    std::fs::write(&out, &blob).expect("write the art pack");
    println!(
        "cargo:warning=art: {} sprites ({} vector glyphs), {} layers, {} curves, {} KiB",
        report.sprites,
        report.vector_glyphs,
        report.layers,
        report.curves,
        blob.len() / 1024
    );
}
