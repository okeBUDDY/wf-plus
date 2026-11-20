//! Cooks the art into the GPU and CPU halves at build time (see wf-art).
//!
//! Inputs: `WF_ART_DIR` (the vector art, one directory per sheet) and
//! `WF_BITMAP_DIR` (the cut original sprites for art that is still drawn
//! as a bitmap). Both default to the copies in `assets/1.08` (see its
//! README). The glyphs are vector too, under `WF_GLYPH_DIR` (default
//! `<art dir>/fonts`, file names from `WF_GLYPH_PATTERNS`, see
//! `wf_cook::GLYPH_PATTERNS`); a glyph with none is blank.

use std::path::PathBuf;

#[allow(dead_code)]
mod shape_table {
    include!("src/shape_table.rs");
}

#[allow(dead_code)]
mod app_info {
    include!("src/app_info.rs");
}

mod windows_resource {
    use super::app_info;
    include!("windows_resource.rs");
}

/// Embeds the version resource, the icon (assets/icon/wf.ico) and the
/// manifest when the target is Windows, on any host: a cross build
/// (x86_64-pc-windows-gnu from Linux) uses MinGW's
/// x86_64-w64-mingw32-windres (`WINDRES` overrides it).
fn embed_icon(manifest: &std::path::Path) {
    let env = |var: &str| std::env::var(var).unwrap_or_default();
    if env("CARGO_CFG_TARGET_OS") != "windows" {
        return;
    }
    let ico = manifest.join("../../assets/icon/wf.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    println!("cargo:rerun-if-changed=src/app_info.rs");
    println!("cargo:rerun-if-changed=windows_resource.rs");
    println!("cargo:rerun-if-env-changed=WINDRES");
    let mut res = windows_resource::windows_resource(&ico.to_string_lossy());
    let prefix = match env("CARGO_CFG_TARGET_ARCH").as_str() {
        "x86_64" => "x86_64-w64-mingw32-",
        "x86" => "i686-w64-mingw32-",
        _ => "",
    };
    if env("HOST") != env("TARGET") && env("CARGO_CFG_TARGET_ENV") == "gnu" && !prefix.is_empty() {
        if std::env::var_os("WINDRES").is_none() {
            res.set_windres_path(&format!("{prefix}windres"));
        }
        if std::env::var_os("AR").is_none() {
            res.set_ar_path(&format!("{prefix}ar"));
        }
    }
    if let Err(e) = res.compile() {
        println!("cargo:warning=windows resource: {e}");
    }
}

fn main() {
    // SDL3's Android hidapi is C++: link the NDK's static C++ runtime.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        // rustc-link-arg: the bin needs it too (rustc-link-lib goes to the lib only).
        println!("cargo:rustc-link-arg=-lc++_static");
        println!("cargo:rustc-link-arg=-lc++abi");
        // SDL's JNI_OnLoad registers its natives with SDLActivity; rustc's
        // version script would hide it in libwfplus.so, so export it.
        println!("cargo:rustc-cdylib-link-arg=-Wl,--undefined=JNI_OnLoad");
        // The loader names it by its file; give it the same soname.
        println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libwfplus.so");
        let script = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("jni.ver");
        std::fs::write(&script, "{ global: JNI_OnLoad; };\n").expect("write the JNI version script");
        println!("cargo:rustc-cdylib-link-arg=-Wl,--version-script={}", script.display());
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    embed_icon(&manifest);
    let game = manifest.join("../../assets/1.08");
    let dir = |var: &str, default: PathBuf| {
        println!("cargo:rerun-if-env-changed={var}");
        std::env::var_os(var).map(PathBuf::from).unwrap_or(default)
    };
    let art_dir = dir("WF_ART_DIR", game.join("sprites/vector"));
    let mut config = wf_cook::Config::new(art_dir.clone(), dir("WF_BITMAP_DIR", game.join("sprites/sheets")));
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
        "cargo:warning=art: {} sprites ({} vector glyphs, {} blank), {} layers, {} curves, {} KiB",
        report.sprites,
        report.vector_glyphs,
        report.blank_glyphs,
        report.layers,
        report.curves,
        blob.len() / 1024
    );
}
