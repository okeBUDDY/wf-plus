//! The cooked art pack.
//!
//! `wf-cook` turns the vector art and glyphs into one blob at build
//! time. The blob has two halves:
//!
//! * the **GPU half**: four texture images whose bytes are uploaded as they
//!   are, with no conversion at load time:
//!   * the curve texture (`Rgba32Float`, [`CURVE_TEX_WIDTH`] texels a row): every
//!     quadratic Bézier is two texels, `(p1.x, p1.y, p2.x, p2.y)` and
//!     `(p3.x, p3.y, 0, 0)`, in the sprite's local coordinates (pixels of the
//!     original art, with the pivot at the origin);
//!   * the band texture (`Rg32Uint`, [`BAND_TEX_WIDTH`] texels a row): for every
//!     layer, its horizontal band headers, then its vertical band headers
//!     (`(curve count, first ref texel)`), then the curve references
//!     (`(curve texel index, 0)`). Horizontal bands are sorted by descending
//!     maximum x and vertical bands by descending maximum y, as the Slug
//!     algorithm expects for its early exit; a fill layer's band then lists
//!     the same curves again by ascending minimum x (y), for the rays cast
//!     the other way;
//!   * the layer texture (`Rgba32Uint`, [`LAYER_TEX_WIDTH`] texels a row):
//!     [`LAYER_TEXELS`] texels per layer, see [`GpuLayer`];
//!   * the ramp texture (`Rgba8Unorm`, [`RAMP_WIDTH`] texels a row): one row per
//!     colour ramp, in straight (not premultiplied) alpha;
//!   * the bitmap atlas (`Rgba8Unorm`, [`BITMAP_TEX_WIDTH`] texels a row,
//!     premultiplied alpha): original pixel art that has no faithful vector
//!     version yet (the title character), drawn as textured quads with
//!     sharp-bilinear sampling. Rects are separated by a transparent texel.
//! * the **CPU half**: the sprite table ([`Sprite`]) and the name strings.
//!
//! All integers and floats are little-endian. Every section starts at a
//! multiple of 16 bytes, so the halves can be borrowed straight out of an
//! `include_bytes!` blob (after alignment, see [`Pack::parse`]).

use bytemuck::{Pod, Zeroable};

pub mod geom;
pub mod shape;
pub mod slug;

pub const MAGIC: [u8; 4] = *b"WFA1";
pub const VERSION: u32 = 3;

pub const CURVE_TEX_WIDTH: u32 = 4096;
pub const BAND_TEX_WIDTH: u32 = 4096;
pub const LAYER_TEX_WIDTH: u32 = 1024;
pub const LAYER_TEXELS: u32 = 8;
pub const RAMP_WIDTH: u32 = 256;
pub const BITMAP_TEX_WIDTH: u32 = 256;

/// Paint kinds of a layer (`GpuLayer::paint`, low byte).
pub mod paint {
    /// `color` straight RGBA.
    pub const SOLID: u32 = 0;
    /// t = projection on `geom[0..2]` -> `geom[2..4]`.
    pub const LINEAR: u32 = 1;
    /// t = distance from `geom[0..2]` / `geom[2]`.
    pub const RADIAL: u32 = 2;

    /// Flag: alpha = base(y) * (1 - depth(y) * (1 - stripe(x))): `ramp` holds
    /// the base in alpha and the depth in red, `ramp2` (geometry `geom2`) the
    /// stripe in alpha (sys0_008).
    pub const FLAG_COMBINE_STRIPE: u32 = 0x100;
    /// Flag: colour = ramp * (1 + amplitude * (noise - 0.52)), `noise` holds
    /// (base frequency, octaves, seed, amplitude) (add0_027).
    pub const FLAG_NOISE: u32 = 0x200;
    /// Flag: the layer's curves are stroke edges (centre points and signed
    /// normals, see [`crate::slug`]), widened in the shader to the
    /// instance's line width in screen pixels.
    pub const FLAG_STROKE: u32 = 0x400;
}

/// How a sprite's layers combine inside the shader before the result is
/// blended into the frame.
pub mod compose {
    /// Source-over, in document order.
    pub const OVER: u32 = 0;
    /// Component-wise maximum (SVG `mix-blend-mode: lighten`).
    pub const MAX: u32 = 1;
}

/// One layer as the shader reads it: [`LAYER_TEXELS`] `Rgba32Uint` texels.
/// Floats are stored as their bits.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct GpuLayer {
    /// Texel 0: the bounding box of the layer's curves (min x, min y, max x,
    /// max y), f32 bits.
    pub bbox: [u32; 4],
    /// Texel 1: first band header texel, horizontal band count, vertical
    /// band count, paint kind | flags.
    pub band_base: u32,
    pub hbands: u32,
    pub vbands: u32,
    pub paint: u32,
    /// Texel 2: the solid colour (straight RGBA, f32 bits), unused by ramps.
    pub color: [u32; 4],
    /// Texel 3: the ramp geometry (f32 bits), see [`paint`].
    pub geom: [u32; 4],
    /// Texel 4: the second ramp's geometry.
    pub geom2: [u32; 4],
    /// Texel 5: ramp row, second ramp row, the heat level (f32 bits; 0 when
    /// the layer is not a heat level), and for [`paint::FLAG_NOISE`] the first
    /// layer texel of the feTurbulence lattice (256 texels of selector,
    /// gradient x and y bits, 0).
    pub ramp: u32,
    pub ramp2: u32,
    pub heat: u32,
    pub reserved5: u32,
    /// Texel 6: noise parameters (f32 bits).
    pub noise: [u32; 4],
    /// Texel 7: band scale and offset: band = (coord - bbox.min) * scale
    /// (x for vertical, y for horizontal bands), f32 bits; then for
    /// [`paint::FLAG_NOISE`] the sprite origin (f32 bits), which takes the
    /// pivot-relative sample back to the SVG user space the noise lives in.
    pub band_scale: [u32; 2],
    pub reserved7: [u32; 2],
}

const _: () = assert!(std::mem::size_of::<GpuLayer>() == (LAYER_TEXELS as usize) * 16);

/// Sprite flags.
pub mod sprite_flags {
    /// The art was drawn additively by the original game (data-blend).
    pub const ADDITIVE_ART: u32 = 0x1;
    /// The sprite is a bitmap in the atlas (`Sprite::bitmap`), not Slug
    /// layers; `bounds` is its quad, one atlas texel per local unit.
    pub const BITMAP: u32 = 0x4;
}

/// One drawable: a list of layers composed into one colour.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Sprite {
    /// Byte range of the name in the string section.
    pub name_offset: u32,
    pub name_len: u32,
    /// First layer (index into the layer texture) and count.
    pub first_layer: u32,
    pub layer_count: u32,
    /// The union of the layers' boxes, in local coordinates.
    pub bounds: [f32; 4],
    /// The size of the original art in pixels (the SVG width and height).
    pub size: [f32; 2],
    /// The pivot in the SVG's own coordinates (already subtracted from all
    /// curve data).
    pub origin: [f32; 2],
    /// [`compose`] mode.
    pub compose: u32,
    /// [`sprite_flags`].
    pub flags: u32,
    /// For [`sprite_flags::BITMAP`]: the atlas rect, x | y << 16 and
    /// width | height << 16.
    pub bitmap: [u32; 2],
}

const _: () = assert!(std::mem::size_of::<Sprite>().is_multiple_of(16));

/// A texture image of the GPU half.
#[derive(Clone, Copy, Debug)]
pub struct Image<'a> {
    pub width: u32,
    pub height: u32,
    pub bytes_per_texel: u32,
    pub data: &'a [u8],
}

/// The file header. Offsets are from the start of the blob.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Header {
    pub magic: [u8; 4],
    pub version: u32,
    pub sprite_count: u32,
    pub layer_count: u32,
    /// (offset, byte length, height in rows) of curves, bands, layers, ramps.
    pub curves: [u32; 4],
    pub bands: [u32; 4],
    pub layers: [u32; 4],
    pub ramps: [u32; 4],
    pub bitmaps: [u32; 4],
    /// (offset, byte length) of the sprite table and the strings.
    pub sprites: [u32; 2],
    pub strings: [u32; 2],
}

/// A parsed pack, borrowing the blob.
pub struct Pack<'a> {
    pub sprites: &'a [Sprite],
    pub layers: &'a [GpuLayer],
    strings: &'a [u8],
    pub curves: Image<'a>,
    pub bands: Image<'a>,
    pub layer_image: Image<'a>,
    pub ramps: Image<'a>,
    pub bitmaps: Image<'a>,
}

#[derive(Debug)]
pub struct FormatError(pub &'static str);

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bad art pack: {}", self.0)
    }
}

impl std::error::Error for FormatError {}

fn section(blob: &[u8], offset: u32, len: u32) -> Result<&[u8], FormatError> {
    blob.get(offset as usize..(offset as usize + len as usize))
        .ok_or(FormatError("section out of range"))
}

impl<'a> Pack<'a> {
    /// Parses `blob`, which must be 16-byte aligned (see [`AlignedBlob`]).
    pub fn parse(blob: &'a [u8]) -> Result<Self, FormatError> {
        if !(blob.as_ptr() as usize).is_multiple_of(16) {
            return Err(FormatError("blob not 16-byte aligned"));
        }
        let header: &Header = bytemuck::try_from_bytes(
            blob.get(..std::mem::size_of::<Header>()).ok_or(FormatError("short header"))?,
        )
        .map_err(|_| FormatError("header"))?;
        if header.magic != MAGIC || header.version != VERSION {
            return Err(FormatError("magic or version"));
        }
        let image = |s: [u32; 4], width: u32, bpt: u32| -> Result<Image<'a>, FormatError> {
            let data = section(blob, s[0], s[1])?;
            if data.len() as u64 != width as u64 * s[2] as u64 * bpt as u64 {
                return Err(FormatError("image size"));
            }
            Ok(Image { width, height: s[2], bytes_per_texel: bpt, data })
        };
        let sprites: &[Sprite] = bytemuck::try_cast_slice(section(blob, header.sprites[0], header.sprites[1])?)
            .map_err(|_| FormatError("sprite table"))?;
        let layer_image = image(header.layers, LAYER_TEX_WIDTH, 16)?;
        let layers: &[GpuLayer] = bytemuck::try_cast_slice(layer_image.data).map_err(|_| FormatError("layers"))?;
        let layers = layers.get(..header.layer_count as usize).ok_or(FormatError("layer count"))?;
        if sprites.len() != header.sprite_count as usize {
            return Err(FormatError("sprite count"));
        }
        Ok(Pack {
            sprites,
            layers,
            strings: section(blob, header.strings[0], header.strings[1])?,
            curves: image(header.curves, CURVE_TEX_WIDTH, 16)?,
            bands: image(header.bands, BAND_TEX_WIDTH, 8)?,
            layer_image,
            ramps: image(header.ramps, RAMP_WIDTH, 4)?,
            bitmaps: image(header.bitmaps, BITMAP_TEX_WIDTH, 4)?,
        })
    }

    pub fn name(&self, sprite: &Sprite) -> &'a str {
        let bytes = &self.strings[sprite.name_offset as usize..(sprite.name_offset + sprite.name_len) as usize];
        std::str::from_utf8(bytes).unwrap_or("")
    }

    /// Looks a sprite up by name (the table is sorted by name).
    pub fn find(&self, name: &str) -> Option<usize> {
        self.sprites.binary_search_by(|s| self.name(s).cmp(name)).ok()
    }
}

/// A byte blob forced to 16-byte alignment, for `include_bytes!`.
#[repr(C, align(16))]
pub struct AlignedBlob<T: ?Sized>(pub T);

/// Includes a file as a 16-byte aligned `&'static [u8]`.
#[macro_export]
macro_rules! include_aligned {
    ($path:expr) => {{
        static BLOB: &$crate::AlignedBlob<[u8]> = &$crate::AlignedBlob(*include_bytes!($path));
        &BLOB.0
    }};
}

/// Writes a pack. Used by the cooker; kept here so the layout lives in one
/// place.
pub struct Writer {
    pub curves: Vec<[f32; 4]>,
    pub bands: Vec<[u32; 2]>,
    pub layers: Vec<GpuLayer>,
    pub ramps: Vec<[[u8; 4]; RAMP_WIDTH as usize]>,
    /// Bitmap atlas texels (premultiplied RGBA8), [`BITMAP_TEX_WIDTH`] a row.
    pub bitmaps: Vec<[u8; 4]>,
    /// (name, sprite); names must be unique.
    pub sprites: Vec<(String, Sprite)>,
}

impl Writer {
    pub fn new() -> Self {
        Writer { curves: Vec::new(), bands: Vec::new(), layers: Vec::new(), ramps: Vec::new(), bitmaps: Vec::new(), sprites: Vec::new() }
    }

    pub fn finish(mut self) -> Vec<u8> {
        fn pad_rows<T: Copy + Default>(v: &mut Vec<T>, width: u32) -> u32 {
            let rows = (v.len() as u32).div_ceil(width).max(1);
            v.resize((rows * width) as usize, T::default());
            rows
        }
        fn align(out: &mut Vec<u8>) {
            while !out.len().is_multiple_of(16) {
                out.push(0);
            }
        }
        let curve_rows = pad_rows(&mut self.curves, CURVE_TEX_WIDTH);
        let band_rows = pad_rows(&mut self.bands, BAND_TEX_WIDTH);
        let bitmap_rows = pad_rows(&mut self.bitmaps, BITMAP_TEX_WIDTH);
        let layer_count = self.layers.len() as u32;
        let layer_rows = (layer_count * LAYER_TEXELS).div_ceil(LAYER_TEX_WIDTH).max(1);
        self.layers.resize((layer_rows * LAYER_TEX_WIDTH / LAYER_TEXELS) as usize, GpuLayer::default());
        if self.ramps.is_empty() {
            self.ramps.push([[0; 4]; RAMP_WIDTH as usize]);
        }
        self.sprites.sort_by(|a, b| a.0.cmp(&b.0));

        let mut strings = Vec::new();
        let mut table = Vec::new();
        for (name, mut sprite) in self.sprites {
            sprite.name_offset = strings.len() as u32;
            sprite.name_len = name.len() as u32;
            strings.extend_from_slice(name.as_bytes());
            table.push(sprite);
        }

        let mut out = vec![0u8; std::mem::size_of::<Header>()];
        align(&mut out);
        let put = |out: &mut Vec<u8>, bytes: &[u8]| -> [u32; 2] {
            align(out);
            let offset = out.len() as u32;
            out.extend_from_slice(bytes);
            [offset, bytes.len() as u32]
        };
        let c = put(&mut out, bytemuck::cast_slice(&self.curves));
        let b = put(&mut out, bytemuck::cast_slice(&self.bands));
        let l = put(&mut out, bytemuck::cast_slice(&self.layers));
        let r = put(&mut out, bytemuck::cast_slice(&self.ramps));
        let bm = put(&mut out, bytemuck::cast_slice(&self.bitmaps));
        let s = put(&mut out, bytemuck::cast_slice(&table));
        let t = put(&mut out, &strings);
        align(&mut out);
        let header = Header {
            magic: MAGIC,
            version: VERSION,
            sprite_count: table.len() as u32,
            layer_count,
            curves: [c[0], c[1], curve_rows, 0],
            bands: [b[0], b[1], band_rows, 0],
            layers: [l[0], l[1], layer_rows, 0],
            ramps: [r[0], r[1], self.ramps.len() as u32, 0],
            bitmaps: [bm[0], bm[1], bitmap_rows, 0],
            sprites: s,
            strings: t,
        };
        out[..std::mem::size_of::<Header>()].copy_from_slice(bytemuck::bytes_of(&header));
        out
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}
