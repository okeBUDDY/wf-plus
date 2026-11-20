//! The wgpu backend of the graphics seam: uploads the cooked art's GPU half
//! once, then turns each [`DrawList`] into Slug instance draws, in order.
//!
//! The curve, band and layer textures have a few rows more than the cooked
//! data: the dynamic region, where the shapes whose points come from game
//! logic ([`Draw::Shape`]) are built with the cooker's band builder, again
//! only when their points change. Every draw, cooked or dynamic, is a Slug
//! instance whose transform is applied on the GPU.

use super::cache::{self, CacheStats, SpriteCache};
use super::timer::GpuTimer;
use super::{Blend, Draw, DrawList, Pass, Primitive};
use std::collections::HashMap;
use wf_art::shape::{Outline, outline, stroke_margin};
use wf_art::slug::{Edges, Target, add_layer, gpu_layer};
use wf_art::{BAND_TEX_WIDTH, CURVE_TEX_WIDTH, GpuLayer, LAYER_TEX_WIDTH, LAYER_TEXELS, Pack, paint};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewUniform {
    scale: [f32; 2],
    offset: [f32; 2],
    time: f32,
    /// Device pixels per logical pixel (the line quads' margin).
    px: f32,
    /// The logical point at the target's top left corner (the fills'
    /// ownership tests work in target pixels).
    origin: [f32; 2],
}

/// A segment of a line shape (`gfx::lines`): its own vertex buffer.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LineInstance {
    seg: [f32; 4],
    nbr: [f32; 4],
    m0: [f32; 4],
    m1: [f32; 2],
    flags: u32,
    tint: [f32; 4],
}

/// A triangle of a fill shape (`gfx::fills`): its own vertex buffer.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FillInstance {
    m0: [f32; 4],
    m1: [f32; 2],
    tint: [f32; 4],
    /// This triangle, the shape's first one (indices into the fill row of
    /// the curve texture, two texels each) and the shape's count.
    tris: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    bounds: [f32; 4],
    m0: [f32; 4],
    m1: [f32; 2],
    tint: [f32; 4],
    layers: [u32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BatchKind {
    Sprites,
    Bitmaps,
    /// Textured quads from a sprite cache page.
    Cached(u16),
    /// Line segments (`gfx::lines`), from the line instances.
    Lines,
    /// Fill triangles (`gfx::fills`), from the fill instances.
    Fills,
}

struct Batch {
    kind: BatchKind,
    blend: Blend,
    pass: Pass,
    start: u32,
    end: u32,
}

/// What the last frame cost, for the perf bench (`Renderer::stats`).
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    /// CPU microseconds: instances and batches (with `dynamic_us`), the
    /// uploads, the encoding and submission.
    pub build_us: f64,
    pub dynamic_us: f64,
    pub upload_us: f64,
    pub submit_us: f64,
    pub draws: u32,
    pub instances: u32,
    pub draw_calls: u32,
    pub pipeline_changes: u32,
    pub upload_bytes: u64,
    /// Dynamic shapes rebuilt (cooked on the CPU) this frame.
    pub dynamic_rebuilt: u32,
    /// Estimated fragment work (only while `collect` is set): pixels inside
    /// the quads, pixels inside the layers' boxes, those times the curves a
    /// pixel's two rays visit, and pixels running the turbulence noise.
    pub quad_px: f64,
    pub layer_px: f64,
    pub curve_px: f64,
    pub noise_px: f64,
    /// Pixels drawn from the sprite cache (one texel fetch each).
    pub cached_px: f64,
    /// Pixels of the analytic shapes: the line segments' quads (inside the
    /// window) and the fills' dilated triangles; their edge tests (four a
    /// line rectangle, up to three rectangles a pixel; three a fill
    /// triangle of the shape, plus one a lower triangle for the ownership).
    pub analytic_px: f64,
    pub analytic_tests: f64,
    /// `curve_px` as it would be with the cache off (every sprite in Slug).
    pub nocache_curve_px: f64,
    /// Rasterizing new cache entries: texels written and the estimated
    /// curve visits.
    pub atlas_px: f64,
    pub atlas_curve_px: f64,
    pub cache: CacheStats,
}

/// Per layer, for the fragment estimate: its box (local units), the
/// curves a pixel's rays visit on average, and whether it has noise.
#[derive(Clone, Copy, Default)]
struct LayerCost {
    bbox: [f32; 4],
    curves: f32,
    noise: bool,
    stroke: bool,
}

/// `bands` is the number of band texels: a layer whose bands do not fit in
/// them is not a shape layer (the layer image also holds the noise
/// lattices, whose texels are not band counts) and costs nothing. Without
/// that check their f32 bits were read as band counts in the billions, and
/// summing them took over a minute at start-up (minutes on a phone: the
/// Android black screen).
fn layer_cost(l: &GpuLayer, bands: u32, band: impl Fn(u32) -> u32) -> LayerCost {
    let end = l.band_base as u64 + l.hbands as u64 + l.vbands as u64;
    if end > bands as u64 {
        return LayerCost::default();
    }
    let mean = |first: u32, n: u32| if n == 0 { 0.0 } else { (first..first + n).map(|i| band(i) as u64).sum::<u64>() as f32 / n as f32 };
    LayerCost {
        bbox: l.bbox.map(f32::from_bits),
        curves: mean(l.band_base, l.hbands) + mean(l.band_base + l.hbands, l.vbands),
        noise: l.paint & paint::FLAG_NOISE != 0,
        stroke: l.paint & paint::FLAG_STROKE != 0,
    }
}

/// Every layer's cost estimate in `art` (at start-up).
fn pack_layer_costs(art: &wf_art::Pack) -> Vec<LayerCost> {
    let band_count = |i: u32| {
        let o = i as usize * 8;
        art.bands.data.get(o..o + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let band_texels = (art.bands.data.len() / 8) as u32;
    art.layers.iter().map(|l| layer_cost(l, band_texels, band_count)).collect()
}

/// The curves a pixel's rays visit on average in each layer of `art`, as
/// the renderer estimates them when it starts (0 for the noise lattices
/// and bitmaps). For tests: it must stay quick on the whole pack.
pub fn layer_curve_estimates(art: &wf_art::Pack) -> Vec<f32> {
    pack_layer_costs(art).iter().map(|c| c.curves).collect()
}

/// Sprite data the CPU needs per draw.
#[derive(Clone, Copy)]
struct SpriteInfo {
    bounds: [f32; 4],
    first_layer: u32,
    layer_count: u32,
    compose: u32,
    /// Atlas rect (x, y, w, h) of a bitmap sprite.
    bitmap: Option<[u32; 4]>,
    /// The curves a pixel of its bounds visits on average (all layers, no
    /// early exit), for the cache's choice and its rasterization budget.
    curves_per_px: f32,
    /// Drawn from the sprite cache when it is on.
    cacheable: bool,
}

/// A sprite is cached when its pixels visit at least this many curves on
/// average: the explosions (hundreds), the bullets and the bosses' parts,
/// not the text (whose glyphs stay Slug-sharp at any size, and are cheap).
pub const CACHE_MIN_CURVES: f32 = 24.0;

/// Estimated curve visits (texels x curves per texel, no early exit, so
/// about three times the real work) the cache may spend rasterizing in one
/// frame: a few hundred microseconds on a phone GPU. A large entry is
/// filled a strip at a time over several frames; its draws use Slug until
/// it is complete.
const RASTER_BUDGET: f64 = 24e6;
/// The smallest strip, rasterized even over the budget when it is the
/// frame's first.
const MIN_STRIP_ROWS: u32 = 16;

/// Sprites kept in Slug whatever their cost: the font glyphs (small text
/// stays exact; the title and HUD art, `etc*`, with its banners and
/// labels, is cached since the four-tap filter keeps its lettering).
fn slug_only(name: &str) -> bool {
    name.starts_with("font/")
}

/// Whether a sprite of `layers` (its layer costs) in `bounds` is worth
/// caching, and its mean curves per pixel.
fn cache_choice(bounds: [f32; 4], layers: &[LayerCost]) -> (f32, bool) {
    let area = |b: [f32; 4]| ((b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0)) as f64;
    let total = area(bounds);
    if total <= 0.0 || layers.is_empty() {
        return (0.0, false);
    }
    let curves = (layers.iter().map(|l| area(l.bbox) * l.curves as f64).sum::<f64>() / total) as f32;
    // Strokes are a screen pixel wide at any scale: not one image per bucket.
    let stroke = layers.iter().any(|l| l.stroke);
    (curves, !stroke && curves >= CACHE_MIN_CURVES)
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    view_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    sprite_pipelines: [wgpu::RenderPipeline; 2],
    bitmap_pipelines: [wgpu::RenderPipeline; 2],
    cached_pipelines: [wgpu::RenderPipeline; 2],
    line_pipelines: [wgpu::RenderPipeline; 2],
    /// The cooked line shapes' segments, by sprite (registered by the game,
    /// which has their points).
    line_sprites: HashMap<u32, Vec<super::lines::Segment>>,
    /// Draw line shapes with `fs_line` and fills of up to `FILL_MAX_TRIS`
    /// triangles with `fs_fill` (else with Slug).
    pub analytic_shapes: bool,
    line_instances: Vec<LineInstance>,
    line_buffer: wgpu::Buffer,
    line_capacity: usize,
    plain_stroke: Vec<bool>,
    fill_pipelines: [wgpu::RenderPipeline; 2],
    fill_instances: Vec<FillInstance>,
    fill_buffer: wgpu::Buffer,
    fill_capacity: usize,
    /// This frame's fill triangles (local points), two texels each.
    fill_texels: Vec<[f32; 4]>,
    /// Slug into a cache page: replaces the texels, never counts.
    atlas_pipeline: wgpu::RenderPipeline,
    /// The Slug bind group with the page's pixel space as the view.
    atlas_bind_group: wgpu::BindGroup,
    pub cache: SpriteCache,
    /// This frame's new cache entries: (page, the Slug instance filling it).
    raster_jobs: Vec<(usize, Instance)>,
    raster_budget: f64,
    timer: Option<GpuTimer>,
    sprites: Vec<SpriteInfo>,
    /// A persistent ring of `RING` instance regions of `instance_capacity`
    /// each: a frame writes its own region, so a write never waits on the
    /// GPU reading the previous frames'.
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    ring_slot: usize,
    // Scratch, reused every frame.
    instances: Vec<Instance>,
    batches: Vec<Batch>,
    dynamic: Dynamic,
    offscreen: Option<(wgpu::TextureView, (u32, u32))>,
    fade: super::fade::FadePass,
    pub time: f32,
    /// The last frame's costs; the fragment estimate only with `collect`.
    pub stats: RenderStats,
    pub collect: bool,
    /// Draws curve visits instead of colours (`new_counting`).
    counting: bool,
    /// With `collect`: estimated curve-pixels per sprite id this frame
    /// (dynamic shapes under u32::MAX).
    pub by_sprite: HashMap<u32, (u32, f64)>,
    /// With `collect`: estimated pixels drawn per sprite id this frame
    /// (quads in the window, whatever draws them; not the analytic shapes).
    pub px_by_sprite: HashMap<u32, f64>,
    costs: Vec<LayerCost>,
}

/// Frames of instance data in flight.
const RING: usize = 3;

/// Rows of the dynamic region: 2048 curves, 8192 band texels, 128 layers.
const DYN_CURVE_ROWS: u32 = 1;
const DYN_BAND_ROWS: u32 = 2;
const DYN_LAYER_ROWS: u32 = 1;
/// The fill triangles of a frame, in the curve texture's row after the
/// dynamic region: two texels each.
const FILL_ROWS: u32 = 1;
const FILL_TRIS: usize = (FILL_ROWS * CURVE_TEX_WIDTH / 2) as usize;
/// Fill shapes of more triangles go to Slug (each pixel tests them all).
const FILL_MAX_TRIS: usize = 16;

/// The line width of strokes in pixels of the 640x480 screen (D3D7's
/// one-pixel lines), whatever the transform.
const LINE_WIDTH: f32 = 1.0;

/// The dynamic region of the curve, band and layer textures.
struct Dynamic {
    curves: wgpu::Texture,
    bands: wgpu::Texture,
    layers: wgpu::Texture,
    /// The first row of the region in each texture.
    curve_row: u32,
    band_row: u32,
    layer_row: u32,
    /// The shapes built in the region: key -> (layer index, bounds).
    built: HashMap<u64, (u32, [f32; 4])>,
    warned: bool,
}

/// Uploads `image` into a texture with `extra` more rows; returns the
/// texture and its first free row.
fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    image: &wf_art::Image,
    format: wgpu::TextureFormat,
    extra: u32,
) -> (wgpu::Texture, u32) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: image.width, height: image.height + extra, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    write_rows(queue, &texture, 0, image.width * image.bytes_per_texel, image.data);
    (texture, image.height)
}

/// Writes whole rows of `bytes` (row length `row_bytes`) from row `row` on.
fn write_rows(queue: &wgpu::Queue, texture: &wgpu::Texture, row: u32, row_bytes: u32, bytes: &[u8]) {
    let rows = (bytes.len() as u32).div_ceil(row_bytes);
    if rows == 0 {
        return;
    }
    let mut data = bytes.to_vec();
    data.resize((rows * row_bytes) as usize, 0);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d { x: 0, y: row, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        &data,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row_bytes), rows_per_image: Some(rows) },
        wgpu::Extent3d { width: texture.width(), height: rows, depth_or_array_layers: 1 },
    );
}

/// The power-of-two bucket of the smallest scale (logical pixels per local
/// unit) of `t`, rounded down: a stroke built for it reaches far enough at
/// that scale or more. None for a degenerate transform.
fn scale_bucket(t: &super::Transform) -> Option<i32> {
    let m = &t.m;
    let sx = (m[0][0] * m[0][0] + m[0][1] * m[0][1]).sqrt();
    let sy = (m[1][0] * m[1][0] + m[1][1] * m[1][1]).sqrt();
    let s = sx.min(sy);
    (s > 1e-6).then(|| s.log2().floor().clamp(-16.0, 16.0) as i32)
}

/// The key of a dynamic shape: its primitive, its points and, for lines, the
/// scale bucket its stroke margin was built for.
fn shape_key(primitive: Primitive, points: &[[f32; 2]], bucket: i32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u32| {
        h ^= v as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    };
    mix(primitive as u32);
    mix(bucket as u32);
    for p in points {
        mix(p[0].to_bits());
        mix(p[1].to_bits());
    }
    h
}

fn blend_state(blend: Blend) -> wgpu::BlendState {
    // The shaders output premultiplied colour.
    let dst = match blend {
        Blend::Normal => wgpu::BlendFactor::OneMinusSrcAlpha,
        Blend::Additive => wgpu::BlendFactor::One,
    };
    let c = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: dst,
        operation: wgpu::BlendOperation::Add,
    };
    wgpu::BlendState { color: c, alpha: c }
}

fn blend_index(blend: Blend) -> usize {
    match blend {
        Blend::Normal => 0,
        Blend::Additive => 1,
    }
}

impl Renderer {
    /// Uploads the art's GPU half and builds the pipelines for `format`.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat, art: &Pack) -> Self {
        Self::build(device, queue, format, art, false)
    }

    /// A renderer for the perf bench that draws, instead of colours, the
    /// curves each pixel's rays visit (red, in 64ths, summed over the draws)
    /// into an `Rgba16Float` target: see [`Renderer::count_visits`].
    pub fn new_counting(device: wgpu::Device, queue: wgpu::Queue, art: &Pack) -> Self {
        Self::build(device, queue, wgpu::TextureFormat::Rgba16Float, art, true)
    }

    fn build(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat, art: &Pack, counting: bool) -> Self {
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let (curve_tex, curve_row) =
            upload(&device, &queue, "slug curves", &art.curves, wgpu::TextureFormat::Rgba32Float, DYN_CURVE_ROWS + FILL_ROWS);
        let (band_tex, band_row) =
            upload(&device, &queue, "slug bands", &art.bands, wgpu::TextureFormat::Rg32Uint, DYN_BAND_ROWS);
        let (layer_tex, layer_row) =
            upload(&device, &queue, "slug layers", &art.layer_image, wgpu::TextureFormat::Rgba32Uint, DYN_LAYER_ROWS);
        let (curves, bands, layers) = (view(&curve_tex), view(&band_tex), view(&layer_tex));
        let ramps = view(&upload(&device, &queue, "ramps", &art.ramps, wgpu::TextureFormat::Rgba8Unorm, 0).0);
        let bitmaps = view(&upload(&device, &queue, "bitmaps", &art.bitmaps, wgpu::TextureFormat::Rgba8Unorm, 0).0);
        let dynamic = Dynamic {
            curves: curve_tex,
            bands: band_tex,
            layers: layer_tex,
            curve_row,
            band_row,
            layer_row,
            built: HashMap::new(),
            warned: false,
        };
        let bitmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sharp bilinear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let view_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view"),
            size: std::mem::size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // The curves are read by vs_fill too (the fill triangles).
        let tex_entry = |binding: u32, sample_type: wgpu::TextureSampleType| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: if binding == 1 { wgpu::ShaderStages::VERTEX_FRAGMENT } else { wgpu::ShaderStages::FRAGMENT },
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("slug"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(1, wgpu::TextureSampleType::Float { filterable: false }),
                tex_entry(2, wgpu::TextureSampleType::Uint),
                tex_entry(3, wgpu::TextureSampleType::Uint),
                tex_entry(4, wgpu::TextureSampleType::Float { filterable: false }),
                tex_entry(5, wgpu::TextureSampleType::Float { filterable: true }),
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let slug_group = |label: &str, view: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: view.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&curves) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&bands) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&layers) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&ramps) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&bitmaps) },
                    wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&bitmap_sampler) },
                ],
            })
        };
        let bind_group = slug_group("slug", &view_buffer);
        // The cache pages' view: page texels -> clip.
        let page_layout = cache::page_layout(&device);
        let cache = SpriteCache::new(&device, page_layout.clone());
        let p = cache.page_size() as f32;
        let atlas_view = ViewUniform { scale: [2.0 / p, -2.0 / p], offset: [-1.0, 1.0], time: 0.0, px: 1.0, origin: [0.0; 2] };
        let atlas_view_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sprite cache view"),
            size: std::mem::size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&atlas_view_buffer, 0, bytemuck::bytes_of(&atlas_view));
        let atlas_bind_group = slug_group("sprite cache slug", &atlas_view_buffer);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("slug.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("slug.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("slug"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let cached_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite cache"),
            bind_group_layouts: &[Some(&layout), Some(&page_layout)],
            immediate_size: 0,
        });
        let instance_attrs =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x2, 3 => Float32x4, 4 => Uint32x4];
        let constants: &[(&str, f64)] = if counting { &[("count_visits", 1.0)] } else { &[] };
        let options = wgpu::PipelineCompilationOptions { constants, ..Default::default() };
        let atlas_options = wgpu::PipelineCompilationOptions::default();
        // `atlas`: Slug into a cache page (its format, no blending, never
        // counting).
        let make_with = |blend: Option<Blend>, kind: BatchKind, atlas: bool| {
            let (vs, fs, topology, buffers) = {
                (
                    match kind {
                        BatchKind::Bitmaps => "vs_bitmap",
                        BatchKind::Sprites => "vs_main",
                        BatchKind::Cached(_) => "vs_cached",
                        BatchKind::Lines | BatchKind::Fills => unreachable!("analytic shapes have their own pipelines"),
                    },
                    match kind {
                        BatchKind::Bitmaps => "fs_bitmap",
                        BatchKind::Sprites => "fs_main",
                        BatchKind::Cached(_) => "fs_cached",
                        BatchKind::Lines | BatchKind::Fills => unreachable!("analytic shapes have their own pipelines"),
                    },
                    wgpu::PrimitiveTopology::TriangleStrip,
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attrs,
                    },
                )
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(match kind {
                    _ if atlas => "sprite cache raster",
                    BatchKind::Bitmaps => "bitmaps",
                    BatchKind::Sprites => "sprites",
                    BatchKind::Cached(_) => "cached sprites",
                    BatchKind::Lines => "lines",
                    BatchKind::Fills => "fills",
                }),
                layout: Some(if matches!(kind, BatchKind::Cached(_)) { &cached_layout } else { &pipeline_layout }),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[Some(buffers)],
                },
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: if atlas { atlas_options.clone() } else { options.clone() },
                    targets: &[Some(wgpu::ColorTargetState {
                        format: if atlas { cache::FORMAT } else { format },
                        blend: blend.map(|b| blend_state(if counting { Blend::Additive } else { b })),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let make = |blend: Blend, kind: BatchKind| make_with(Some(blend), kind, false);
        let sprite_pipelines = [make(Blend::Normal, BatchKind::Sprites), make(Blend::Additive, BatchKind::Sprites)];
        let bitmap_pipelines = [make(Blend::Normal, BatchKind::Bitmaps), make(Blend::Additive, BatchKind::Bitmaps)];
        let cached_pipelines = [make(Blend::Normal, BatchKind::Cached(0)), make(Blend::Additive, BatchKind::Cached(0))];
        let atlas_pipeline = make_with(None, BatchKind::Sprites, true);
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x2, 4 => Uint32, 5 => Float32x4];
        let fill_attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x2, 2 => Float32x4, 3 => Uint32x4];
        let make_analytic = |blend: Blend, fill: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if fill { "fills" } else { "lines" }),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(if fill { "vs_fill" } else { "vs_line" }),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: if fill { std::mem::size_of::<FillInstance>() } else { std::mem::size_of::<LineInstance>() } as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: if fill { &fill_attrs } else { &line_attrs },
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: if fill { wgpu::PrimitiveTopology::TriangleList } else { wgpu::PrimitiveTopology::TriangleStrip },
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if fill { "fs_fill" } else { "fs_line" }),
                    compilation_options: options.clone(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend_state(if counting { Blend::Additive } else { blend })),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let line_pipelines = [make_analytic(Blend::Normal, false), make_analytic(Blend::Additive, false)];
        let fill_pipelines = [make_analytic(Blend::Normal, true), make_analytic(Blend::Additive, true)];
        let costs = pack_layer_costs(art);
        let sprites = art
            .sprites
            .iter()
            .map(|s| {
                let bitmap = (s.flags & wf_art::sprite_flags::BITMAP != 0)
                    .then(|| [s.bitmap[0] & 0xffff, s.bitmap[0] >> 16, s.bitmap[1] & 0xffff, s.bitmap[1] >> 16]);
                let layers = costs.get(s.first_layer as usize..(s.first_layer + s.layer_count) as usize).unwrap_or(&[]);
                let (curves_per_px, cacheable) = cache_choice(s.bounds, layers);
                SpriteInfo {
                    bounds: s.bounds,
                    first_layer: s.first_layer,
                    layer_count: s.layer_count,
                    compose: s.compose,
                    bitmap,
                    curves_per_px,
                    cacheable: cacheable && bitmap.is_none() && s.flags & wf_art::sprite_flags::TRACED == 0 && !slug_only(art.name(s)),
                }
            })
            .collect();
        let timer = GpuTimer::new(&device, &queue);
        let line_capacity = 256;
        let line_buffer = Self::buffer(&device, "line instances", RING * line_capacity * std::mem::size_of::<LineInstance>());
        let fill_capacity = 64;
        let fill_buffer = Self::buffer(&device, "fill instances", RING * fill_capacity * std::mem::size_of::<FillInstance>());
        // Which sprites are one plain stroke layer (white, solid): those a
        // registered line shape may draw with `fs_line`.
        let plain_stroke: Vec<bool> = art
            .sprites
            .iter()
            .map(|s| {
                s.layer_count == 1
                    && s.flags & wf_art::sprite_flags::BITMAP == 0
                    && art.layers.get(s.first_layer as usize).is_some_and(|l| {
                        l.paint == paint::SOLID | paint::FLAG_STROKE && l.color.map(f32::from_bits) == [1.0; 4]
                    })
            })
            .collect();
        let instance_capacity = 1024;
        let instance_buffer = Self::buffer(&device, "instances", RING * instance_capacity * std::mem::size_of::<Instance>());
        let fade = super::fade::FadePass::new(&device, format);
        Renderer {
            device,
            queue,
            format,
            view_buffer,
            bind_group,
            sprite_pipelines,
            bitmap_pipelines,
            cached_pipelines,
            line_pipelines,
            line_sprites: HashMap::new(),
            plain_stroke,
            analytic_shapes: true,
            line_instances: Vec::new(),
            line_buffer,
            line_capacity,
            fill_pipelines,
            fill_instances: Vec::new(),
            fill_buffer,
            fill_capacity,
            fill_texels: Vec::new(),
            atlas_pipeline,
            atlas_bind_group,
            cache,
            raster_jobs: Vec::new(),
            raster_budget: 0.0,
            timer,
            sprites,
            instance_buffer,
            instance_capacity,
            ring_slot: 0,
            instances: Vec::new(),
            batches: Vec::new(),
            dynamic,
            offscreen: None,
            time: 0.0,
            fade,
            stats: RenderStats::default(),
            collect: false,
            counting,
            by_sprite: HashMap::new(),
            px_by_sprite: HashMap::new(),
            costs,
        }
    }

    fn buffer(device: &wgpu::Device, label: &str, size: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: size as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    fn push_batch(&mut self, kind: BatchKind, blend: Blend, pass: Pass, start: u32, end: u32) {
        if let Some(last) = self.batches.last_mut()
            && last.kind == kind
            && last.blend == blend
            && last.pass == pass
            && last.end == start
        {
            last.end = end;
            return;
        }
        self.batches.push(Batch { kind, blend, pass, start, end });
    }

    /// The cache entry to draw sprite `id` from under `m` (logical units)
    /// at `k` device pixels per logical one: the existing one, or a new one
    /// queued for rasterization this frame within the frame's budget. None:
    /// draw it with Slug.
    fn cached(&mut self, id: u32, info: SpriteInfo, m: &[[f32; 3]; 3], k: f32) -> Option<cache::Entry> {
        if !self.cache.enabled || !info.cacheable {
            return None;
        }
        let s = cache::max_scale([m[0][0], m[0][1], m[1][0], m[1][1]]) * k;
        if s.is_nan() || s <= 1e-3 {
            return None;
        }
        let mut n = cache::bucket(s * cache::OVERSAMPLE);
        // Too large for a page's half: less oversampling, down to the
        // screen's own scale (then Slug).
        let max = self.cache.max_entry();
        while cache::entry_size(info.bounds, cache::bucket_scale(n)).0.max(cache::entry_size(info.bounds, cache::bucket_scale(n)).1) > max
            && cache::bucket_scale(n - 1) >= s * 0.999
        {
            n -= 1;
        }
        let e = match self.cache.get(id, n) {
            Some(e) => e,
            None if self.raster_budget <= 0.0 => {
                self.cache.stats.deferred += 1;
                return None;
            }
            None => match self.cache.insert(id, n, info.bounds) {
                Some(e) => e,
                None => {
                    self.cache.stats.deferred += 1;
                    return None;
                }
            },
        };
        if e.complete() {
            self.cache.stats.hits += 1;
            return Some(e);
        }
        // Rasterize the next rows the frame's budget pays for (at least a
        // strip when nothing else was this frame, so every entry finishes).
        let [_, _, w, h] = e.rect;
        let row_work = w as f64 * info.curves_per_px.max(1.0) as f64;
        let mut rows = (self.raster_budget / row_work).floor().max(0.0) as u32;
        if self.raster_jobs.is_empty() {
            rows = rows.max(MIN_STRIP_ROWS);
        }
        let rows = rows.min(h - e.done);
        if rows == 0 {
            self.cache.stats.deferred += 1;
            return None;
        }
        let work = rows as f64 * row_work;
        self.raster_budget -= work;
        self.stats.atlas_px += (rows * w) as f64;
        self.stats.atlas_curve_px += work;
        self.cache.stats.rasterized += 1;
        let scale = e.scale;
        let o = e.origin;
        self.raster_jobs.push((
            e.page,
            Instance {
                bounds: e.strip(e.done, e.done + rows),
                m0: [scale, 0.0, 0.0, scale],
                m1: [e.rect[0] as f32 - o[0] * scale, e.rect[1] as f32 - o[1] * scale],
                tint: [1.0; 4],
                layers: [info.first_layer, info.layer_count, info.compose, LINE_WIDTH.to_bits()],
            },
        ));
        let done = e.done + rows;
        self.cache.set_done(id, n, done);
        if done < h {
            self.cache.stats.deferred += 1;
            return None;
        }
        // Complete: the raster passes run before the frame's, so it is
        // drawn from this frame.
        self.cache.stats.hits += 1;
        Some(cache::Entry { done, ..e })
    }

    /// Registers cooked sprite `id` as the line shape `points` drawn as
    /// `primitive`, so it is drawn with `fs_line`. Ignored unless the sprite
    /// is one plain stroke layer and the shape is a line.
    pub fn set_line_sprite(&mut self, id: u32, primitive: Primitive, points: &[[f32; 2]]) {
        let segs = super::lines::segments(primitive, points);
        if self.plain_stroke.get(id as usize).copied().unwrap_or(false) && !segs.is_empty() {
            self.line_sprites.insert(id, segs);
        }
    }

    /// Whether sprite `id` is drawn with `fs_line`.
    pub fn is_line_sprite(&self, id: u32) -> bool {
        self.line_sprites.contains_key(&id)
    }

    /// Queues the line instances of `segs` under `m`, `color`.
    #[allow(clippy::too_many_arguments)]
    fn push_lines(&mut self, segs: &[super::lines::Segment], m: &[[f32; 3]; 3], color: super::Argb, blend: Blend, pass: Pass, layout: &super::Layout, size: (u32, u32)) {
        let start = self.line_instances.len() as u32;
        for s in segs {
            let nan = f32::NAN;
            let (na, nb) = (s.nbr_a.unwrap_or([nan; 2]), s.nbr_b.unwrap_or([nan; 2]));
            self.line_instances.push(LineInstance {
                seg: [s.a[0], s.a[1], s.b[0], s.b[1]],
                nbr: [na[0], na[1], nb[0], nb[1]],
                m0: [m[0][0], m[0][1], m[1][0], m[1][1]],
                m1: [m[2][0], m[2][1]],
                flags: s.flags(),
                tint: color.to_f32(),
            });
            if self.collect {
                let px = line_area(s, m, layout, size);
                self.stats.analytic_px += px;
                self.stats.analytic_tests += px * 4.0 * (1 + s.nbr_a.is_some() as u32 + s.nbr_b.is_some() as u32) as f64;
            }
        }
        let end = self.line_instances.len() as u32;
        if end > start {
            self.push_batch(BatchKind::Lines, blend, pass, start, end);
        }
    }

    /// Queues the fill triangles `tris` (oriented as Slug's) under `m`,
    /// `color`: one instance each, sharing the shape's triangles in the
    /// fill row. False (nothing queued) when the row is full.
    #[allow(clippy::too_many_arguments)]
    fn push_fill(&mut self, tris: &[[[f32; 2]; 3]], m: &[[f32; 3]; 3], color: super::Argb, blend: Blend, pass: Pass, layout: &super::Layout, size: (u32, u32)) -> bool {
        let first = self.fill_texels.len() / 2;
        if first + tris.len() > FILL_TRIS {
            return false;
        }
        let row = (self.dynamic.curve_row + DYN_CURVE_ROWS) * CURVE_TEX_WIDTH / 2;
        let start = self.fill_instances.len() as u32;
        let shared = super::fills::shared_edges(tris);
        for (i, t) in tris.iter().enumerate() {
            self.fill_texels.push([t[0][0], t[0][1], t[1][0], t[1][1]]);
            self.fill_texels.push([t[2][0], t[2][1], shared[i] as f32, 0.0]);
            self.fill_instances.push(FillInstance {
                m0: [m[0][0], m[0][1], m[1][0], m[1][1]],
                m1: [m[2][0], m[2][1]],
                tint: color.to_f32(),
                tris: [row + (first + i) as u32, row + first as u32, tris.len() as u32, 0],
            });
            if self.collect {
                let px = super::fills::area(t, m, layout, size);
                self.stats.analytic_px += px;
                self.stats.analytic_tests += px * (3 * tris.len() + i + 1) as f64;
            }
        }
        self.push_batch(BatchKind::Fills, blend, pass, start, self.fill_instances.len() as u32);
        true
    }

    /// Whether sprite `id` is drawn from the cache when it is on.
    pub fn sprite_cacheable(&self, id: u32) -> bool {
        self.sprites.get(id as usize).is_some_and(|s| s.cacheable)
    }

    /// Turns the sprite cache on or off (off frees its pages).
    pub fn set_sprite_cache(&mut self, on: bool) {
        self.cache.enabled = on;
        if !on {
            self.cache.clear();
            self.cache.pages.clear();
        }
    }

    /// The GPU time of a frame (ms, the mean over the last ~30 timed
    /// frames) when the device has timestamp queries.
    pub fn gpu_ms(&self) -> Option<f32> {
        self.timer.as_ref().and_then(|t| t.mean_ms)
    }

    /// The last timed frame's GPU time (ms).
    pub fn last_gpu_ms(&self) -> Option<f32> {
        self.timer.as_ref().and_then(|t| t.last_ms)
    }

    /// Whether GPU timestamps are measured.
    pub fn has_gpu_timer(&self) -> bool {
        self.timer.is_some()
    }

    /// Waits for the GPU and reads the timestamps of the frames submitted
    /// (the benchmarks: `last_gpu_ms` is then this frame's).
    pub fn collect_gpu_time(&mut self) {
        if let Some(t) = &mut self.timer {
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
            t.collect();
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
            t.collect();
        }
    }

    /// Builds the frame's dynamic shapes into the dynamic region, unless all
    /// of them are already there (their points did not change).
    fn build_dynamic(&mut self, list: &DrawList) {
        let mut wanted: Vec<(u64, Primitive, &[[f32; 2]], i32)> = Vec::new();
        // The fills the frame draws analytically (as the render loop
        // decides: in order, while the fill row has room).
        let mut fill_tris = 0;
        for draw in &list.draws {
            if let Draw::Shape { primitive, points, transform, .. } = draw {
                if self.analytic_shapes && primitive.is_line() && !self.collect {
                    continue;
                }
                if self.analytic_shapes && !primitive.is_line() && scale_bucket(transform).is_some() {
                    let n = super::fills::triangles(*primitive, points).len();
                    if n <= FILL_MAX_TRIS && fill_tris + n <= FILL_TRIS {
                        fill_tris += n;
                        if !self.collect {
                            continue;
                        }
                    }
                }
                let Some(bucket) = scale_bucket(transform) else {
                    continue;
                };
                let bucket = if primitive.is_line() { bucket } else { 0 };
                let key = shape_key(*primitive, points, bucket);
                if !wanted.iter().any(|w| w.0 == key) {
                    wanted.push((key, *primitive, points, bucket));
                }
            }
        }
        if wanted.iter().all(|w| self.dynamic.built.contains_key(&w.0)) {
            return;
        }
        let rebuilt = wanted.iter().filter(|w| !self.dynamic.built.contains_key(&w.0)).count() as u32;
        self.stats.dynamic_rebuilt += rebuilt;
        let mut new_costs = Vec::new();
        let dy = &mut self.dynamic;
        dy.built.clear();
        let (mut curves, mut bands, mut layers) = (Vec::new(), Vec::new(), Vec::<GpuLayer>::new());
        let layers_per_row = LAYER_TEX_WIDTH / LAYER_TEXELS;
        for (key, primitive, points, bucket) in wanted {
            let mut target = Target {
                curves: &mut curves,
                bands: &mut bands,
                curve_base: dy.curve_row * CURVE_TEX_WIDTH,
                band_base: dy.band_row * BAND_TEX_WIDTH,
            };
            let (g, flags) = match outline(primitive, points) {
                Outline::Fill(c) => (add_layer(&mut target, Edges::Fill(&c)), 0),
                Outline::Stroke(e) => {
                    (add_layer(&mut target, Edges::Stroke(&e, stroke_margin(2f32.powi(bucket)))), paint::FLAG_STROKE)
                }
            };
            if curves.len() as u32 > DYN_CURVE_ROWS * CURVE_TEX_WIDTH
                || bands.len() as u32 > DYN_BAND_ROWS * BAND_TEX_WIDTH
                || layers.len() as u32 >= DYN_LAYER_ROWS * layers_per_row
            {
                if !dy.warned {
                    crate::log::warn("gfx: the dynamic shapes do not fit their region; some are not drawn");
                    dy.warned = true;
                }
                break;
            }
            let mut layer = gpu_layer(&g);
            layer.paint = paint::SOLID | flags;
            layer.color = [1.0f32; 4].map(f32::to_bits);
            let base = dy.band_row * BAND_TEX_WIDTH;
            new_costs.push((dy.layer_row * layers_per_row + layers.len() as u32, layer_cost(&layer, base + bands.len() as u32, |i| {
                bands.get(i.wrapping_sub(base) as usize).map_or(0, |b: &[u32; 2]| b[0])
            })));
            dy.built.insert(key, (dy.layer_row * layers_per_row + layers.len() as u32, g.bbox));
            layers.push(layer);
        }
        let row_bytes = |n: usize, row: u32| (n as u32).div_ceil(row) as u64 * row as u64;
        self.stats.upload_bytes += row_bytes(curves.len() * 16, CURVE_TEX_WIDTH * 16)
            + row_bytes(bands.len() * 8, BAND_TEX_WIDTH * 8)
            + row_bytes(layers.len() * 128, LAYER_TEX_WIDTH * 16);
        write_rows(&self.queue, &dy.curves, dy.curve_row, CURVE_TEX_WIDTH * 16, bytemuck::cast_slice(&curves));
        write_rows(&self.queue, &dy.bands, dy.band_row, BAND_TEX_WIDTH * 8, bytemuck::cast_slice(&bands));
        write_rows(&self.queue, &dy.layers, dy.layer_row, LAYER_TEX_WIDTH * 16, bytemuck::cast_slice(&layers));
        for (i, c) in new_costs {
            if self.costs.len() <= i as usize {
                self.costs.resize(i as usize + 1, LayerCost::default());
            }
            self.costs[i as usize] = c;
        }
    }

}

/// The device pixels of a line segment's quad inside the window: its
/// length clipped to the window (Liang-Barsky) times its width with the
/// antialiasing margin.
fn line_area(s: &super::lines::Segment, m: &[[f32; 3]; 3], layout: &super::Layout, size: (u32, u32)) -> f64 {
    let k = layout.scale;
    let dev = |p: [f32; 2]| {
        [
            (p[0] * m[0][0] + p[1] * m[1][0] + m[2][0] - layout.visible[0]) * k,
            (p[0] * m[0][1] + p[1] * m[1][1] + m[2][1] - layout.visible[1]) * k,
        ]
    };
    let (a, b) = (dev(s.a), dev(s.b));
    let d = [b[0] - a[0], b[1] - a[1]];
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-d[0], a[0]), (d[0], size.0 as f32 - a[0]), (-d[1], a[1]), (d[1], size.1 as f32 - a[1])] {
        if p == 0.0 {
            if q < 0.0 {
                return 0.0;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    if t1 <= t0 {
        return 0.0;
    }
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt() * (t1 - t0);
    ((len + 3.0) * (k + 3.0)) as f64
}

/// An instance's estimated fragment work (pixels).
#[derive(Clone, Copy, Default)]
struct Cost {
    quad: f64,
    layer: f64,
    curve: f64,
    noise: f64,
}

impl Cost {
    fn add_to(self, stats: &mut RenderStats) {
        stats.quad_px += self.quad;
        stats.layer_px += self.layer;
        stats.curve_px += self.curve;
        stats.noise_px += self.noise;
    }
}

/// An instance's estimated fragment work: each area in pixels, scaled by
/// the part of its screen box inside the window.
#[allow(clippy::too_many_arguments)]
fn estimate(costs: &[LayerCost], m: &[[f32; 3]; 3], layout: &super::Layout, size: (u32, u32), bounds: [f32; 4], first: u32, count: u32) -> Cost {
    let mut cost = Cost::default();
    let k = layout.scale;
    let det = ((m[0][0] * m[1][1] - m[0][1] * m[1][0]).abs() * k * k) as f64;
    let area = |b: [f32; 4]| -> f64 {
        let full = ((b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0)) as f64 * det;
        if full <= 0.0 {
            return 0.0;
        }
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for (x, y) in [(b[0], b[1]), (b[2], b[1]), (b[0], b[3]), (b[2], b[3])] {
            let p = [
                (x * m[0][0] + y * m[1][0] + m[2][0] - layout.visible[0]) * k,
                (x * m[0][1] + y * m[1][1] + m[2][1] - layout.visible[1]) * k,
            ];
            for a in 0..2 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
        let boxed = ((hi[0] - lo[0]) * (hi[1] - lo[1])) as f64;
        let clipped = ((hi[0].min(size.0 as f32) - lo[0].max(0.0)).max(0.0) * (hi[1].min(size.1 as f32) - lo[1].max(0.0)).max(0.0)) as f64;
        if boxed <= 0.0 { 0.0 } else { full * clipped / boxed }
    };
    cost.quad = area(bounds);
    for l in first..first + count {
        let Some(c) = costs.get(l as usize) else { continue };
        let px = area(c.bbox);
        cost.layer += px;
        cost.curve += px * c.curves as f64;
        if c.noise {
            cost.noise += px;
        }
    }
    cost
}

impl Renderer {
    /// The layers of sprite `id` for the perf bench: (curves a pixel's rays
    /// visit, noise, box).
    pub fn layer_costs(&self, id: u32) -> Vec<(f32, bool, [f32; 4])> {
        let Some(s) = self.sprites.get(id as usize) else { return Vec::new() };
        (s.first_layer..s.first_layer + s.layer_count)
            .filter_map(|l| self.costs.get(l as usize))
            .map(|c| (c.curves, c.noise, c.bbox))
            .collect()
    }

    /// Renders `list` into `target` (of `size` pixels): the arena fitted
    /// and centred in the window, the background pass filling the margins
    /// around it.
    pub fn render(&mut self, list: &DrawList, target: &wgpu::TextureView, size: (u32, u32)) {
        self.render_for(list, target, size, size);
    }

    /// Renders `list` as for a window of `window` pixels into a `target` of
    /// `size` pixels (a render scale's): the window's logical view at the
    /// target's resolution.
    pub fn render_for(&mut self, list: &DrawList, target: &wgpu::TextureView, size: (u32, u32), window: (u32, u32)) {
        let layout = super::Layout::new(list.arena, window).resized(window, size);
        let k = layout.scale;

        let t0 = std::time::Instant::now();
        self.stats = RenderStats { draws: list.draws.len() as u32, ..Default::default() };
        self.by_sprite.clear();
        self.px_by_sprite.clear();
        self.instances.clear();
        self.batches.clear();
        self.raster_jobs.clear();
        self.line_instances.clear();
        self.fill_instances.clear();
        self.fill_texels.clear();
        self.raster_budget = RASTER_BUDGET;
        self.cache.begin_frame();
        if let Some(t) = &mut self.timer {
            t.begin_frame(&self.device);
        }
        self.build_dynamic(list);
        self.stats.dynamic_us = t0.elapsed().as_secs_f64() * 1e6;
        for draw in &list.draws {
            match draw {
                Draw::Sprite { sprite, transform, color, blend, clip, pass, .. } => {
                    let Some(&info) = self.sprites.get(sprite.0 as usize) else {
                        continue;
                    };
                    if self.analytic_shapes
                        && clip.is_none()
                        && let Some(segs) = self.line_sprites.get(&sprite.0)
                    {
                        let segs = segs.clone();
                        if self.collect {
                            let c = estimate(&self.costs, &transform.m, &layout, size, info.bounds, info.first_layer, info.layer_count);
                            self.stats.nocache_curve_px += c.curve;
                            self.by_sprite.entry(sprite.0).or_default().0 += 1;
                        }
                        self.push_lines(&segs, &transform.m, *color, *blend, *pass, &layout, size);
                        continue;
                    }
                    if let Some(rect) = info.bitmap {
                        let m = &transform.m;
                        if self.collect {
                            *self.px_by_sprite.entry(sprite.0).or_default() += estimate(&self.costs, m, &layout, size, info.bounds, 0, 0).quad;
                        }
                        let i = self.instances.len() as u32;
                        self.instances.push(Instance {
                            bounds: info.bounds,
                            m0: [m[0][0], m[0][1], m[1][0], m[1][1]],
                            m1: [m[2][0], m[2][1]],
                            tint: color.to_f32(),
                            layers: rect,
                        });
                        self.push_batch(BatchKind::Bitmaps, *blend, *pass, i, i + 1);
                        continue;
                    }
                    if info.layer_count == 0 {
                        continue;
                    }
                    let m = &transform.m;
                    let sx = (m[0][0] * m[0][0] + m[0][1] * m[0][1]).sqrt();
                    let sy = (m[1][0] * m[1][0] + m[1][1] * m[1][1]).sqrt();
                    let scale = sx.min(sy) * k;
                    if scale <= 1e-6 {
                        continue;
                    }
                    // Dilate the quad by one device pixel so the edge pixels
                    // get their antialiasing.
                    let d = 1.0 / scale;
                    let b = info.bounds;
                    let mut bounds = [b[0] - d, b[1] - d, b[2] + d, b[3] + d];
                    if let Some(c) = clip {
                        // A texel region: hard edges, as the original quad.
                        bounds = [bounds[0].max(c[0]), bounds[1].max(c[1]), bounds[2].min(c[2]), bounds[3].min(c[3])];
                        if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
                            continue;
                        }
                    }
                    let cost = if self.collect {
                        let c = estimate(&self.costs, m, &layout, size, bounds, info.first_layer, info.layer_count);
                        self.stats.nocache_curve_px += c.curve;
                        Some(c)
                    } else {
                        None
                    };
                    if let Some(e) = self.cached(sprite.0, info, m, k) {
                        let (mut quad, _) = e.quad();
                        if let Some(c) = clip {
                            // The texel region of a sheet: cut the quad, hard
                            // edges as the Slug path's.
                            quad = [quad[0].max(c[0]), quad[1].max(c[1]), quad[2].min(c[2]), quad[3].min(c[3])];
                        }
                        let t0 = e.texel_of([quad[0], quad[1]]);
                        let texels = [t0[0].to_bits(), t0[1].to_bits(), e.scale.to_bits(), 0];
                        if self.collect {
                            let px = estimate(&self.costs, m, &layout, size, quad, 0, 0).quad;
                            self.stats.cached_px += px;
                            self.by_sprite.entry(sprite.0).or_default().0 += 1;
                            *self.px_by_sprite.entry(sprite.0).or_default() += px;
                        }
                        let i = self.instances.len() as u32;
                        self.instances.push(Instance {
                            bounds: quad,
                            m0: [m[0][0], m[0][1], m[1][0], m[1][1]],
                            m1: [m[2][0], m[2][1]],
                            tint: color.to_f32(),
                            layers: texels,
                        });
                        self.push_batch(BatchKind::Cached(e.page as u16), *blend, *pass, i, i + 1);
                        continue;
                    }
                    if let Some(c) = cost {
                        c.add_to(&mut self.stats);
                        let e = self.by_sprite.entry(sprite.0).or_default();
                        e.0 += 1;
                        e.1 += c.curve;
                        *self.px_by_sprite.entry(sprite.0).or_default() += c.quad;
                    }
                    let i = self.instances.len() as u32;
                    self.instances.push(Instance {
                        bounds,
                        m0: [m[0][0], m[0][1], m[1][0], m[1][1]],
                        m1: [m[2][0], m[2][1]],
                        tint: color.to_f32(),
                        layers: [info.first_layer, info.layer_count, info.compose, LINE_WIDTH.to_bits()],
                    });
                    self.push_batch(BatchKind::Sprites, *blend, *pass, i, i + 1);
                }
                Draw::Shape { primitive, points, transform, color, blend, pass, .. } if self.analytic_shapes && primitive.is_line() => {
                    if self.collect {
                        // The Slug cost it replaces (its layer when built).
                        if let Some(bucket) = scale_bucket(transform)
                            && let Some(&(layer, b)) = self.dynamic.built.get(&shape_key(*primitive, points, bucket))
                        {
                            let c = estimate(&self.costs, &transform.m, &layout, size, b, layer, 1);
                            self.stats.nocache_curve_px += c.curve;
                        }
                        self.by_sprite.entry(u32::MAX - 1).or_default().0 += 1;
                    }
                    let segs = super::lines::segments(*primitive, points);
                    self.push_lines(&segs, &transform.m, *color, *blend, *pass, &layout, size);
                }
                Draw::Shape { primitive, points, transform, color, blend, pass, .. }
                    if self.analytic_shapes
                        && scale_bucket(transform).is_some()
                        && let tris = super::fills::triangles(*primitive, points)
                        && tris.len() <= FILL_MAX_TRIS
                        && self.fill_texels.len() / 2 + tris.len() <= FILL_TRIS =>
                {
                    if self.collect {
                        if let Some(&(layer, b)) = self.dynamic.built.get(&shape_key(*primitive, points, 0)) {
                            let c = estimate(&self.costs, &transform.m, &layout, size, b, layer, 1);
                            self.stats.nocache_curve_px += c.curve;
                        }
                        self.by_sprite.entry(u32::MAX - 1).or_default().0 += 1;
                    }
                    self.push_fill(&tris, &transform.m, *color, *blend, *pass, &layout, size);
                }
                Draw::Shape { primitive, points, transform, color, blend, pass, .. } => {
                    let m = &transform.m;
                    let Some(bucket) = scale_bucket(transform) else {
                        continue;
                    };
                    let key = shape_key(*primitive, points, if primitive.is_line() { bucket } else { 0 });
                    let Some(&(layer, b)) = self.dynamic.built.get(&key) else {
                        continue;
                    };
                    let sx = (m[0][0] * m[0][0] + m[0][1] * m[0][1]).sqrt();
                    let sy = (m[1][0] * m[1][0] + m[1][1] * m[1][1]).sqrt();
                    let d = 1.0 / (sx.min(sy) * k).max(1e-6);
                    if self.collect {
                        let c = estimate(&self.costs, m, &layout, size, [b[0] - d, b[1] - d, b[2] + d, b[3] + d], layer, 1);
                        c.add_to(&mut self.stats);
                        self.stats.nocache_curve_px += c.curve;
                        let e = self.by_sprite.entry(u32::MAX).or_default();
                        e.0 += 1;
                        e.1 += c.curve;
                    }
                    let i = self.instances.len() as u32;
                    self.instances.push(Instance {
                        bounds: [b[0] - d, b[1] - d, b[2] + d, b[3] + d],
                        m0: [m[0][0], m[0][1], m[1][0], m[1][1]],
                        m1: [m[2][0], m[2][1]],
                        tint: color.to_f32(),
                        layers: [layer, 1, 0, LINE_WIDTH.to_bits()],
                    });
                    self.push_batch(BatchKind::Sprites, *blend, *pass, i, i + 1);
                }
            }
        }

        // The new cache entries' Slug instances follow the frame's, by page.
        self.raster_jobs.sort_by_key(|j| j.0);
        let raster_start = self.instances.len() as u32;
        self.instances.extend(self.raster_jobs.iter().map(|j| j.1));
        self.stats.cache = self.cache.stats;
        self.stats.build_us = t0.elapsed().as_secs_f64() * 1e6;
        let t1 = std::time::Instant::now();
        if self.instances.len() > self.instance_capacity {
            self.instance_capacity = self.instances.len().next_power_of_two();
            self.instance_buffer =
                Self::buffer(&self.device, "instances", RING * self.instance_capacity * std::mem::size_of::<Instance>());
        }
        self.ring_slot = (self.ring_slot + 1) % RING;
        let ring_offset = (self.ring_slot * self.instance_capacity * std::mem::size_of::<Instance>()) as u64;
        if !self.instances.is_empty() {
            self.queue.write_buffer(&self.instance_buffer, ring_offset, bytemuck::cast_slice(&self.instances));
        }
        if self.line_instances.len() > self.line_capacity {
            self.line_capacity = self.line_instances.len().next_power_of_two();
            self.line_buffer = Self::buffer(&self.device, "line instances", RING * self.line_capacity * std::mem::size_of::<LineInstance>());
        }
        let line_offset = (self.ring_slot * self.line_capacity * std::mem::size_of::<LineInstance>()) as u64;
        if !self.line_instances.is_empty() {
            self.queue.write_buffer(&self.line_buffer, line_offset, bytemuck::cast_slice(&self.line_instances));
        }
        if self.fill_instances.len() > self.fill_capacity {
            self.fill_capacity = self.fill_instances.len().next_power_of_two();
            self.fill_buffer = Self::buffer(&self.device, "fill instances", RING * self.fill_capacity * std::mem::size_of::<FillInstance>());
        }
        let fill_offset = (self.ring_slot * self.fill_capacity * std::mem::size_of::<FillInstance>()) as u64;
        if !self.fill_instances.is_empty() {
            self.queue.write_buffer(&self.fill_buffer, fill_offset, bytemuck::cast_slice(&self.fill_instances));
            // Only the texels used: a part of the fill row.
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.dynamic.curves,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: self.dynamic.curve_row + DYN_CURVE_ROWS, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&self.fill_texels),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: None, rows_per_image: None },
                wgpu::Extent3d { width: self.fill_texels.len() as u32, height: 1, depth_or_array_layers: 1 },
            );
            self.stats.upload_bytes += (self.fill_texels.len() * 16) as u64;
        }
        // Logical -> clip over the whole window (the visible rect).
        let [x0, y0, w, h] = layout.visible;
        let view = ViewUniform {
            scale: [2.0 / w, -2.0 / h],
            offset: [-1.0 - x0 * 2.0 / w, 1.0 + y0 * 2.0 / h],
            time: self.time,
            px: k,
            origin: [x0, y0],
        };
        self.queue.write_buffer(&self.view_buffer, 0, bytemuck::bytes_of(&view));
        let fading = list.fade.0 >> 24 != 0 && !self.counting;
        if fading {
            self.fade.prepare(&self.queue, list.fade.to_f32());
        }
        self.stats.instances = self.instances.len() as u32;
        self.stats.upload_bytes += (self.instances.len() * std::mem::size_of::<Instance>()
            + self.line_instances.len() * std::mem::size_of::<LineInstance>()
            + std::mem::size_of::<ViewUniform>()) as u64;
        self.stats.upload_us = t1.elapsed().as_secs_f64() * 1e6;
        let t2 = std::time::Instant::now();

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        let instances = self.instance_buffer.slice(ring_offset..);
        // Fill the new cache entries first: one pass per page written.
        let mut first_pass = true;
        let mut j = 0;
        while j < self.raster_jobs.len() {
            let page = self.raster_jobs[j].0;
            let end = j + self.raster_jobs[j..].iter().take_while(|r| r.0 == page).count();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sprite cache"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.cache.pages[page].view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: self.timer.as_ref().and_then(|t| t.writes(first_pass, false)),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            first_pass = false;
            pass.set_pipeline(&self.atlas_pipeline);
            pass.set_bind_group(0, &self.atlas_bind_group, &[]);
            pass.set_vertex_buffer(0, instances);
            pass.draw(0..4, raster_start + j as u32..raster_start + end as u32);
            self.stats.draw_calls += 1;
            j = end;
        }
        {
            let clear = list.clear.to_f32();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear[0] as f64,
                            g: clear[1] as f64,
                            b: clear[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: self.timer.as_ref().and_then(|t| t.writes(first_pass, true)),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            let [ax, ay, aw, ah] = if list.menu { [0, 0, size.0, size.1] } else { layout.arena_px };
            let mut scissor = None;
            let mut pipeline = None;
            for batch in &self.batches {
                self.stats.draw_calls += 1;
                if pipeline != Some((batch.kind, batch.blend)) {
                    self.stats.pipeline_changes += 1;
                    pipeline = Some((batch.kind, batch.blend));
                }
                if scissor != Some(batch.pass) {
                    match batch.pass {
                        Pass::Background | Pass::Overlay => pass.set_scissor_rect(0, 0, size.0.max(1), size.1.max(1)),
                        Pass::Arena => pass.set_scissor_rect(ax, ay, aw.max(1), ah.max(1)),
                    }
                    scissor = Some(batch.pass);
                }
                match batch.kind {
                    BatchKind::Sprites => {
                        pass.set_pipeline(&self.sprite_pipelines[blend_index(batch.blend)]);
                        pass.set_vertex_buffer(0, instances);
                        pass.draw(0..4, batch.start..batch.end);
                    }
                    BatchKind::Bitmaps => {
                        pass.set_pipeline(&self.bitmap_pipelines[blend_index(batch.blend)]);
                        pass.set_vertex_buffer(0, instances);
                        pass.draw(0..4, batch.start..batch.end);
                    }
                    BatchKind::Lines => {
                        pass.set_pipeline(&self.line_pipelines[blend_index(batch.blend)]);
                        pass.set_vertex_buffer(0, self.line_buffer.slice(line_offset..));
                        pass.draw(0..4, batch.start..batch.end);
                    }
                    BatchKind::Fills => {
                        pass.set_pipeline(&self.fill_pipelines[blend_index(batch.blend)]);
                        pass.set_vertex_buffer(0, self.fill_buffer.slice(fill_offset..));
                        pass.draw(0..3, batch.start..batch.end);
                    }
                    BatchKind::Cached(page) => {
                        pass.set_pipeline(&self.cached_pipelines[blend_index(batch.blend)]);
                        pass.set_bind_group(1, &self.cache.pages[page as usize].bind_group, &[]);
                        pass.set_vertex_buffer(0, instances);
                        pass.draw(0..4, batch.start..batch.end);
                    }
                }
            }
            if fading {
                self.fade.draw(&mut pass, size);
            }
        }
        if let Some(t) = &self.timer {
            t.resolve(&mut encoder);
        }
        self.queue.submit([encoder.finish()]);
        if let Some(t) = &mut self.timer {
            t.after_submit();
        }
        self.stats.submit_us = t2.elapsed().as_secs_f64() * 1e6;
    }

    /// Renders `list` into an offscreen target of `size` kept between calls,
    /// and waits for the GPU (for benchmarks).
    pub fn render_offscreen(&mut self, list: &DrawList, size: (u32, u32)) {
        let stale = self.offscreen.as_ref().is_none_or(|(_, s)| *s != size);
        if stale {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("offscreen"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.offscreen = Some((texture.create_view(&wgpu::TextureViewDescriptor::default()), size));
        }
        let view = self.offscreen.as_ref().map(|(v, _)| v.clone()).expect("offscreen target");
        self.render(list, &view, size);
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// A counting renderer's frame: the curves visited by all pixels'
    /// rays, summed (the measured fragment work), and the sprite cache's
    /// texel fetches.
    pub fn count_visits(&mut self, list: &DrawList, size: (u32, u32)) -> (f64, f64, f64) {
        let (width, height) = size;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("visits"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut black = list.clone();
        black.clear = super::Argb(0xff00_0000);
        self.render(&black, &view, size);
        let row = (width * 8).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("visits readback"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("visits") });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("map visits"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        let data = buffer.slice(..).get_mapped_range().expect("mapped visits");
        let (mut sum, mut fetches, mut edges) = (0.0, 0.0, 0.0);
        for y in 0..height {
            for x in 0..width {
                let o = (y * row + x * 8) as usize;
                sum += f16_to_f32(u16::from_le_bytes([data[o], data[o + 1]])) as f64 * 64.0;
                fetches += f16_to_f32(u16::from_le_bytes([data[o + 2], data[o + 3]])) as f64 * 64.0;
                edges += f16_to_f32(u16::from_le_bytes([data[o + 4], data[o + 5]])) as f64 * 64.0;
            }
        }
        (sum, fetches, edges)
    }

    /// Renders `list` into a new `width` x `height` image and reads it back
    /// as RGBA8 (for tests and screenshots).
    pub fn render_to_rgba(&mut self, list: &DrawList, width: u32, height: u32) -> Vec<u8> {
        self.render_to_rgba_with(width, height, |r, view| r.render(list, view, (width, height)))
    }

    /// Reads back as RGBA8 a new `width` x `height` image that `draw`
    /// renders into.
    pub fn render_to_rgba_with(&mut self, width: u32, height: u32, draw: impl FnOnce(&mut Self, &wgpu::TextureView)) -> Vec<u8> {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        draw(self, &view);
        let row = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        let data = buffer.slice(..).get_mapped_range().expect("mapped readback");
        let bgra = matches!(self.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let line = &data[(y * row) as usize..(y * row + width * 4) as usize];
            for p in line.chunks(4) {
                if bgra {
                    out.extend_from_slice(&[p[2], p[1], p[0], 255]);
                } else {
                    out.extend_from_slice(&[p[0], p[1], p[2], 255]);
                }
            }
        }
        out
    }
}

fn f16_to_f32(h: u16) -> f32 {
    let (sign, exp, frac) = ((h >> 15) as u32, ((h >> 10) & 0x1f) as i32, (h & 0x3ff) as f32);
    let v = match exp {
        0 => frac * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1.0 + frac / 1024.0) * 2f32.powi(exp - 15),
    };
    if sign == 1 { -v } else { v }
}

/// Creates a device for `surface` (or a headless one when `None`).
pub fn create_device(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface>,
) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), String> {
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: surface,
        apply_limit_buckets: false,
    }))
    .map_err(|e| format!("no GPU adapter: {e}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("wf"),
        // Frame timing where the adapter has it (Vulkan on Android does).
        required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .map_err(|e| format!("cannot create the GPU device: {e}"))?;
    Ok((adapter, device, queue))
}
