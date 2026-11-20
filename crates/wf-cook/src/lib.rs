//! The art cooker. `cook()` reads the vector art and glyphs and
//! returns a [`wf_art`] pack. The game's build script calls it, so the game
//! only ever sees cooked data.

pub mod geom;
pub mod slug;
pub mod svg;
pub mod turbulence;

use std::path::{Path, PathBuf};

use geom::Vec2;
use wf_art::{GpuLayer, RAMP_WIDTH, Sprite, Writer, compose, paint, sprite_flags};

/// The sprite sheets of the vector art, one directory each.
pub const SHEETS: &[&str] = &["add0", "enm0", "etc0", "etc1", "etc2", "sys0"];

/// Art the cooker leaves out: the title character (drawn from its original
/// pixels, see [`BITMAPS`]) and the artist's spacing helpers
/// (`*_extra00`), which are not game art. Listed so the build works
/// whether or not those files exist.
pub const SKIPPED: &[&str] = &["etc2_001_character", "add0_extra00", "enm0_extra00"];

/// Where vector glyphs are looked for, relative to `Config::glyph_dir`,
/// first match wins. `{font}` is the grid name (`text`, `kana`, ...),
/// `{sheet}` the font sheet (`font0`..`font2`), `{index}` the glyph in its
/// grid (3 digits), `{x}`/`{y}` the cell's pixel position on the sheet.
/// The SVG is in cell coordinates (the glyph's top-left corner at 0, 0). A
/// glyph with no SVG is blank (the sheets' empty cells, such as the space).
///
/// `{csv}` is the glyph's file in `Config::glyph_csv` (the cut glyphs'
/// table, `fonts/glyphs.csv`: `file, sheet, ..., x, y, w, h`) with `.svg`
/// for `.png`, e.g. `ascii_small16/ascii_small16_01_U+0021.svg`.
pub const GLYPH_PATTERNS: &[&str] = &["{csv}", "{font}/{font}_{index}.svg", "{sheet}/{sheet}_{x}_{y}.svg", "{sheet}_{x}_{y}.svg"];

/// Original pixel art cooked as bitmaps (no faithful vector version yet):
/// (sprite name, PNG under `Config::bitmap_dir`, pivot in the PNG).
///
/// The title character is the left 55 px of etc2 pattern 1, whose pivot is
/// (160, 56); keeping that pivot lets it be drawn with the logo's transform.
pub const BITMAPS: &[(&str, &str, [f32; 2])] = &[("etc2_001_character", "etc2/etc2_001_character.png", [160.0, 56.0])];

/// A font grid as the game sets it up (cGlobal::LoadFonts, 0x401280).
pub struct FontGrid {
    pub name: &'static str,
    pub sheet: &'static str,
    pub count: usize,
    pub u: usize,
    pub v: usize,
    pub width: usize,
    pub height: usize,
    pub columns: usize,
}

pub const FONTS: &[FontGrid] = &[
    FontGrid { name: "text", sheet: "font1", count: 64, u: 0, v: 192, width: 16, height: 16, columns: 16 },
    FontGrid { name: "small_text", sheet: "font0", count: 64, u: 128, v: 64, width: 8, height: 8, columns: 16 },
    FontGrid { name: "kana", sheet: "font1", count: 96, u: 0, v: 0, width: 16, height: 32, columns: 16 },
    FontGrid { name: "title", sheet: "font0", count: 28, u: 0, v: 0, width: 20, height: 32, columns: 12 },
    FontGrid { name: "large_digit", sheet: "font0", count: 10, u: 0, v: 96, width: 48, height: 80, columns: 5 },
    FontGrid { name: "digit", sheet: "font2", count: 10, u: 0, v: 48, width: 24, height: 40, columns: 5 },
    // The three 128 x 16 caption strips at the top of font2, which the
    // HUD, title and options draw as scrolling bars (DrawTexels, wrapped).
    FontGrid { name: "strip", sheet: "font2", count: 3, u: 0, v: 0, width: 128, height: 16, columns: 1 },
];

/// The name of glyph `index` of `font` in the pack.
pub fn glyph_name(font: &str, index: usize) -> String {
    format!("font/{font}/{index:03}")
}

pub struct Config {
    /// The vector art: `<dir>/<sheet>/<sheet>_<id>.svg`.
    pub art_dir: PathBuf,
    /// The cut original sprites (`sprites/sheets`), for [`BITMAPS`].
    pub bitmap_dir: PathBuf,
    /// The vector glyphs.
    pub glyph_dir: PathBuf,
    /// File name patterns under `glyph_dir` (see [`GLYPH_PATTERNS`]).
    pub glyph_patterns: Vec<String>,
    /// The table of the cut glyphs, for the `{csv}` pattern.
    pub glyph_csv: PathBuf,
    /// The game's constant shapes, cooked as `shape/<name>`.
    pub shapes: &'static [wf_art::shape::ShapeDef],
}

/// Splits one CSV line (fields may be quoted, with "" for a quote).
fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                fields.last_mut().unwrap().push('"');
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(String::new()),
            _ => fields.last_mut().unwrap().push(c),
        }
    }
    fields
}

/// The cut glyphs by (sheet, x, y): their file without the extension.
fn read_glyph_csv(path: &Path) -> std::collections::HashMap<(String, usize, usize), String> {
    let mut map = std::collections::HashMap::new();
    let Ok(text) = std::fs::read_to_string(path) else { return map };
    for line in text.lines().skip(1) {
        let f = csv_fields(line);
        if f.len() < 11 || f[0].is_empty() {
            continue;
        }
        let (Ok(x), Ok(y)) = (f[7].parse(), f[8].parse()) else { continue };
        let sheet = f[1].trim_end_matches(".png").to_string();
        map.insert((sheet, x, y), f[0].trim_end_matches(".png").to_string());
    }
    map
}

impl Config {
    /// The usual layout: glyphs under `<art_dir>/fonts`.
    pub fn new(art_dir: PathBuf, bitmap_dir: PathBuf) -> Self {
        let glyph_dir = art_dir.join("fonts");
        let glyph_csv = art_dir.join("../fonts/glyphs.csv");
        Config { art_dir, bitmap_dir, glyph_dir, glyph_patterns: GLYPH_PATTERNS.iter().map(|s| s.to_string()).collect(), glyph_csv, shapes: &[] }
    }

    /// The vector glyph for the cell at (`x`, `y`) of `grid`, if there is
    /// one.
    fn glyph_svg(&self, csv: &std::collections::HashMap<(String, usize, usize), String>, grid: &FontGrid, index: usize, x: usize, y: usize) -> Option<PathBuf> {
        let cut = csv.get(&(grid.sheet.to_string(), x, y));
        self.glyph_patterns.iter().filter_map(|p| {
            if p.contains("{csv}") {
                return cut.map(|c| self.glyph_dir.join(p.replace("{csv}", &format!("{c}.svg"))));
            }
            Some(
            self.glyph_dir.join(
                p.replace("{font}", grid.name)
                    .replace("{sheet}", grid.sheet)
                    .replace("{index}", &format!("{index:03}"))
                    .replace("{x}", &x.to_string())
                    .replace("{y}", &y.to_string()),
            ))
        }).find(|p| p.is_file())
    }
}

#[derive(Default)]
pub struct Report {
    pub inputs: Vec<PathBuf>,
    pub sprites: usize,
    pub layers: usize,
    pub curves: usize,
    pub bitmaps: usize,
    pub skipped: Vec<String>,
    /// Glyphs cooked from vector SVGs.
    pub vector_glyphs: usize,
    /// Glyphs with no SVG, cooked blank.
    pub blank_glyphs: usize,
    /// Directories whose contents decide the output (for rerun-if-changed).
    pub dirs: Vec<PathBuf>,
    /// Inputs that were skipped or replaced, for the build to print.
    pub warnings: Vec<String>,
}

pub type Error = String;

struct Cooker {
    w: Writer,
    /// Shelf packer state of the bitmap atlas: x, shelf top, shelf height.
    shelf: (usize, usize, usize),
    ramp_rows: Vec<[[u8; 4]; RAMP_WIDTH as usize]>,
    /// feTurbulence lattices already in the layer texture: (seed bits, first
    /// texel).
    noise_tables: Vec<(u32, u32)>,
    report: Report,
}

fn ramp_row(stops: &[(f32, [f32; 4])]) -> [[u8; 4]; RAMP_WIDTH as usize] {
    let mut row = [[0u8; 4]; RAMP_WIDTH as usize];
    for (i, texel) in row.iter_mut().enumerate() {
        let t = (i as f32 + 0.5) / RAMP_WIDTH as f32;
        let c = if stops.is_empty() {
            [0.0; 4]
        } else if t <= stops[0].0 {
            stops[0].1
        } else if t >= stops[stops.len() - 1].0 {
            stops[stops.len() - 1].1
        } else {
            let k = stops.windows(2).position(|w| t >= w[0].0 && t <= w[1].0).unwrap_or(0);
            let (a, b) = (stops[k], stops[k + 1]);
            let f = if b.0 > a.0 { (t - a.0) / (b.0 - a.0) } else { 0.0 };
            std::array::from_fn(|j| a.1[j] + (b.1[j] - a.1[j]) * f)
        };
        *texel = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    row
}

fn bits4(v: [f32; 4]) -> [u32; 4] {
    v.map(f32::to_bits)
}

impl Cooker {
    fn ramp(&mut self, stops: &[(f32, [f32; 4])]) -> u32 {
        let row = ramp_row(stops);
        if let Some(i) = self.ramp_rows.iter().position(|r| *r == row) {
            return i as u32;
        }
        self.ramp_rows.push(row);
        (self.ramp_rows.len() - 1) as u32
    }

    fn ramp_geom(geom: &svg::RampGeom, origin: Vec2) -> (u32, [f32; 4]) {
        match geom {
            svg::RampGeom::Linear(a, b) => (paint::LINEAR, [a.x - origin.x, a.y - origin.y, b.x - origin.x, b.y - origin.y]),
            svg::RampGeom::Radial(c, r) => (paint::RADIAL, [c.x - origin.x, c.y - origin.y, *r, 0.0]),
        }
    }

    fn add_sprite(&mut self, name: String, layers: Vec<GpuLayer>, size: [f32; 2], origin: Vec2, compose: u32, flags: u32) {
        let first_layer = self.w.layers.len() as u32;
        let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for l in &layers {
            let b = l.bbox.map(f32::from_bits);
            bounds = [bounds[0].min(b[0]), bounds[1].min(b[1]), bounds[2].max(b[2]), bounds[3].max(b[3])];
        }
        if layers.is_empty() {
            bounds = [0.0; 4];
        }
        self.report.layers += layers.len();
        self.w.layers.extend(layers.iter().copied());
        self.w.sprites.push((
            name,
            Sprite {
                first_layer,
                layer_count: layers.len() as u32,
                bounds,
                size,
                origin: [origin.x, origin.y],
                compose,
                flags,
                ..Default::default()
            },
        ));
        self.report.sprites += 1;
    }

    /// The first layer texel of the feTurbulence lattice for `seed`, baked on
    /// first use as whole (unreferenced) layers ahead of the sprite's own.
    fn noise_table(&mut self, seed: f32) -> u32 {
        if let Some(&(_, base)) = self.noise_tables.iter().find(|t| t.0 == seed.to_bits()) {
            return base;
        }
        let base = self.w.layers.len() as u32 * wf_art::LAYER_TEXELS;
        let texels = turbulence::NoiseTable::new(seed).texels();
        for chunk in texels.chunks(wf_art::LAYER_TEXELS as usize) {
            let mut raw = [[0u32; 4]; wf_art::LAYER_TEXELS as usize];
            raw[..chunk.len()].copy_from_slice(chunk);
            self.w.layers.push(bytemuck::cast(raw));
        }
        self.noise_tables.push((seed.to_bits(), base));
        base
    }

    /// A constant shape of the game: one white layer, filled or stroked,
    /// in the shape's own coordinates.
    fn cook_shape(&mut self, def: &wf_art::shape::ShapeDef) {
        use wf_art::shape::{Outline, outline, stroke_margin};
        let (g, flags) = match outline(def.primitive, def.points) {
            Outline::Fill(curves) => {
                self.report.curves += curves.len();
                (slug::add_layer(&mut self.w, &curves), 0)
            }
            Outline::Stroke(edges) => {
                self.report.curves += edges.len();
                (slug::add_edges(&mut self.w, slug::Edges::Stroke(&edges, stroke_margin(def.min_scale))), paint::FLAG_STROKE)
            }
        };
        let mut layer = slug::gpu_layer(&g);
        layer.paint = paint::SOLID | flags;
        layer.color = bits4([1.0; 4]);
        self.add_sprite(format!("shape/{}", def.name), vec![layer], [0.0, 0.0], Vec2::new(0.0, 0.0), compose::OVER, 0);
    }

    fn cook_svg(&mut self, name: String, art: svg::Art) {
        let mut layers = Vec::new();
        for path in &art.paths {
            let curves: Vec<_> = path.curves.iter().map(|c| c.offset(art.origin)).collect();
            self.report.curves += curves.len();
            let g = slug::add_layer(&mut self.w, &curves);
            let mut layer = slug::gpu_layer(&g);
            match &path.paint {
                svg::Paint::Solid(c) => {
                    layer.paint = paint::SOLID;
                    layer.color = bits4(*c);
                }
                svg::Paint::Ramp(r) => {
                    let (kind, geom) = Self::ramp_geom(&r.geom, art.origin);
                    layer.paint = kind;
                    layer.geom = bits4(geom);
                    layer.ramp = self.ramp(&r.stops);
                }
            }
            if let Some(stripe) = &art.combine_stripe {
                let (_, geom2) = Self::ramp_geom(&stripe.geom, art.origin);
                layer.paint |= paint::FLAG_COMBINE_STRIPE;
                layer.geom2 = bits4(geom2);
                layer.ramp2 = self.ramp(&stripe.stops);
            }
            if let Some(n) = &art.noise {
                layer.paint |= paint::FLAG_NOISE;
                layer.noise = bits4([n.frequency, n.octaves, n.seed, n.amplitude]);
                layer.reserved5 = self.noise_table(n.seed);
                // feTurbulence samples user space, the curves are pivot-relative.
                layer.reserved7 = [art.origin.x.to_bits(), art.origin.y.to_bits()];
            }
            layer.heat = path.heat.unwrap_or(0.0).to_bits();
            layers.push(layer);
        }
        let flags = if art.additive { sprite_flags::ADDITIVE_ART } else { 0 };
        let mode = if art.layers_max { compose::MAX } else { compose::OVER };
        self.add_sprite(name, layers, [art.width, art.height], art.origin, mode, flags);
    }

    /// Puts `w` x `h` straight-RGBA pixels into the atlas (premultiplied),
    /// with a transparent texel around them. Returns the texel rect origin.
    fn place_bitmap(&mut self, width: usize, height: usize, rgba: &[u8]) -> (usize, usize) {
        let aw = wf_art::BITMAP_TEX_WIDTH as usize;
        assert!(width + 2 <= aw, "bitmap wider than the atlas");
        let (mut x, mut top, mut shelf_h) = self.shelf;
        if x + width + 2 > aw {
            top += shelf_h;
            x = 0;
            shelf_h = 0;
        }
        shelf_h = shelf_h.max(height + 2);
        let rows = top + shelf_h;
        if self.w.bitmaps.len() < rows * aw {
            self.w.bitmaps.resize(rows * aw, [0; 4]);
        }
        for y in 0..height {
            for xx in 0..width {
                let p = &rgba[(y * width + xx) * 4..][..4];
                let a = p[3] as u32;
                let pm = |c: u8| ((c as u32 * a + 127) / 255) as u8;
                self.w.bitmaps[(top + 1 + y) * aw + x + 1 + xx] = [pm(p[0]), pm(p[1]), pm(p[2]), p[3]];
            }
        }
        self.shelf = (x + width + 2, top, shelf_h);
        (x + 1, top + 1)
    }

    fn cook_bitmap(&mut self, name: &str, width: usize, height: usize, rgba: &[u8], origin: [f32; 2]) {
        let (ax, ay) = self.place_bitmap(width, height, rgba);
        let bounds = [-origin[0], -origin[1], width as f32 - origin[0], height as f32 - origin[1]];
        self.w.sprites.push((
            name.to_string(),
            Sprite {
                bounds,
                size: [width as f32, height as f32],
                origin,
                flags: sprite_flags::BITMAP,
                bitmap: [(ax | ay << 16) as u32, (width | height << 16) as u32],
                ..Default::default()
            },
        ));
        self.report.sprites += 1;
        self.report.bitmaps += 1;
    }

    fn cook_font(&mut self, config: &Config, csv: &std::collections::HashMap<(String, usize, usize), String>, grid: &FontGrid) {
        for index in 0..grid.count {
            let (x, y) = (grid.u + index % grid.columns * grid.width, grid.v + index / grid.columns * grid.height);
            let art = config.glyph_svg(csv, grid, index, x, y).and_then(|path| {
                self.report.inputs.push(path.clone());
                // A glyph SVG that cannot be read or parsed is cooked blank,
                // with a warning.
                std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|text| svg::parse(&text).map_err(|e| e.to_string()))
                    .map_err(|e| self.report.warnings.push(format!("{}: {e}; blank instead", path.display())))
                    .ok()
            });
            let Some(mut art) = art else {
                // Glyphs are placed by their top-left corner.
                self.add_sprite(glyph_name(grid.name, index), Vec::new(), [grid.width as f32, grid.height as f32], Vec2::new(0.0, 0.0), compose::OVER, 0);
                self.report.blank_glyphs += 1;
                continue;
            };
            art.origin = Vec2::new(0.0, 0.0);
            art.width = grid.width as f32;
            art.height = grid.height as f32;
            self.cook_svg(glyph_name(grid.name, index), art);
            self.report.vector_glyphs += 1;
        }
    }
}

fn read_png(path: &Path) -> Result<(usize, usize, Vec<u8>), Error> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("png too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..w * h * 4].to_vec(),
        png::ColorType::Rgb => buf[..w * h * 3].chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..w * h * 2].chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf[..w * h].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        other => return Err(format!("{}: unsupported colour type {other:?}", path.display())),
    };
    Ok((w, h, rgba))
}

pub fn cook(config: &Config) -> Result<(Vec<u8>, Report), Error> {
    let mut c = Cooker { w: Writer::new(), shelf: (0, 0, 0), ramp_rows: Vec::new(), noise_tables: Vec::new(), report: Report::default() };
    for def in config.shapes {
        c.cook_shape(def);
    }
    for sheet in SHEETS {
        let dir = config.art_dir.join(sheet);
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "svg"))
            .collect();
        files.sort();
        for path in files {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            c.report.inputs.push(path.clone());
            if SKIPPED.contains(&name.as_str()) {
                c.report.skipped.push(name);
                continue;
            }
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let art = svg::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            c.cook_svg(name, art);
        }
    }
    c.report.dirs.push(config.glyph_dir.clone());
    let csv = read_glyph_csv(&config.glyph_csv);
    if config.glyph_csv.is_file() {
        c.report.inputs.push(config.glyph_csv.clone());
    }
    for grid in FONTS {
        c.cook_font(config, &csv, grid);
    }
    for (name, file, origin) in BITMAPS {
        let path = config.bitmap_dir.join(file);
        c.report.inputs.push(path.clone());
        let (width, height, rgba) = read_png(&path)?;
        c.cook_bitmap(name, width, height, &rgba, *origin);
    }
    c.w.ramps = std::mem::take(&mut c.ramp_rows);
    let report = std::mem::take(&mut c.report);
    Ok((c.w.finish(), report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_patterns_find_vector_glyphs() {
        let dir = std::env::temp_dir().join(format!("wf-cook-glyphs-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("digit")).unwrap();
        std::fs::write(dir.join("digit/digit_003.svg"), "<svg/>").unwrap();
        std::fs::write(dir.join("font0_48_96.svg"), "<svg/>").unwrap();
        std::fs::create_dir_all(dir.join("strips")).unwrap();
        std::fs::write(dir.join("strips/font2_strip2.svg"), "<svg/>").unwrap();
        let mut config = Config::new(PathBuf::new(), PathBuf::new());
        config.glyph_dir = dir.clone();
        let csv: std::collections::HashMap<_, _> = [(("font2".to_string(), 0, 32), "strips/font2_strip2".to_string())].into();
        let strip = FONTS.iter().find(|g| g.name == "strip").unwrap();
        assert_eq!(config.glyph_svg(&csv, strip, 2, 0, 32), Some(dir.join("strips/font2_strip2.svg")));
        let digit = FONTS.iter().find(|g| g.name == "digit").unwrap();
        let large = FONTS.iter().find(|g| g.name == "large_digit").unwrap();
        assert_eq!(config.glyph_svg(&csv, digit, 3, 72, 48), Some(dir.join("digit/digit_003.svg")));
        assert_eq!(config.glyph_svg(&csv, digit, 4, 96, 48), None);
        assert_eq!(config.glyph_svg(&csv, large, 1, 48, 96), Some(dir.join("font0_48_96.svg")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn csv_lines_split_with_quotes() {
        assert_eq!(csv_fields(r#"a/b.png,font1.png,x,2,"""",U+0022,22,32,192,16,16"#)[4], "\"");
        assert_eq!(csv_fields(r#"f,font1.png,x,12,",",U+002C"#)[4], ",");
    }

    #[test]
    fn spacing_helpers_are_skipped() {
        assert!(SKIPPED.contains(&"add0_extra00") && SKIPPED.contains(&"enm0_extra00"));
    }
}
