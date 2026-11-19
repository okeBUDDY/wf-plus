//! Cooks the art into the GPU and CPU halves at build time (see wf-art).
//!
//! Inputs: `WF_ART_DIR` (the vector art, one directory per sheet) and
//! `WF_FONT_DIR` (font0.png .. font2.png). Both default to the project's
//! shared folder next to this repository. `WF_BITMAP_DIR` holds the cut
//! original sprites for art that is still drawn as a bitmap.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let game = manifest.join("../../../software/warning-forever/1.08");
    let dir = |var: &str, default: PathBuf| {
        println!("cargo:rerun-if-env-changed={var}");
        std::env::var_os(var).map(PathBuf::from).unwrap_or(default)
    };
    let config = wf_cook::Config {
        art_dir: dir("WF_ART_DIR", game.join("sprites/vector")),
        font_dir: dir("WF_FONT_DIR", game.join("data")),
        bitmap_dir: dir("WF_BITMAP_DIR", game.join("sprites/sheets")),
    };
    let (blob, report) = wf_cook::cook(&config).unwrap_or_else(|e| panic!("cooking the art failed: {e}"));
    for input in &report.inputs {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("art.wfa");
    std::fs::write(&out, &blob).expect("write the art pack");
    println!(
        "cargo:warning=art: {} sprites, {} layers, {} curves, {} KiB",
        report.sprites,
        report.layers,
        report.curves,
        blob.len() / 1024
    );
}
