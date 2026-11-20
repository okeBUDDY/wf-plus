//! The wgpu backend of the graphics seam: uploads the cooked art's GPU half
//! once, then turns each [`DrawList`] into Slug instance draws, in order.
//!
//! The curve, band and layer textures have a few rows more than the cooked
//! data: the dynamic region, where the shapes whose points come from game
//! logic ([`Draw::Shape`]) are built with the cooker's band builder, again
//! only when their points change. Every draw, cooked or dynamic, is a Slug
//! instance whose transform is applied on the GPU.

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
    pad: [f32; 3],
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
}

/// Per layer, for the fragment estimate: its box (local units), the
/// curves a pixel's rays visit on average, and whether it has noise.
#[derive(Clone, Copy, Default)]
struct LayerCost {
    bbox: [f32; 4],
    curves: f32,
    noise: bool,
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
    }
}

/// Sprite data the CPU needs per draw.
struct SpriteInfo {
    bounds: [f32; 4],
    first_layer: u32,
    layer_count: u32,
    compose: u32,
    /// Atlas rect (x, y, w, h) of a bitmap sprite.
    bitmap: Option<[u32; 4]>,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    view_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    sprite_pipelines: [wgpu::RenderPipeline; 2],
    bitmap_pipelines: [wgpu::RenderPipeline; 2],
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
    costs: Vec<LayerCost>,
}

/// Frames of instance data in flight.
const RING: usize = 3;

/// Rows of the dynamic region: 2048 curves, 8192 band texels, 128 layers.
const DYN_CURVE_ROWS: u32 = 1;
const DYN_BAND_ROWS: u32 = 2;
const DYN_LAYER_ROWS: u32 = 1;

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
            upload(&device, &queue, "slug curves", &art.curves, wgpu::TextureFormat::Rgba32Float, DYN_CURVE_ROWS);
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
        let tex_entry = |binding: u32, sample_type: wgpu::TextureSampleType| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
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
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("slug"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: view_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&curves) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&bands) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&layers) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&ramps) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&bitmaps) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&bitmap_sampler) },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("slug.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("slug.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("slug"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let instance_attrs =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x2, 3 => Float32x4, 4 => Uint32x4];
        let constants: &[(&str, f64)] = if counting { &[("count_visits", 1.0)] } else { &[] };
        let options = wgpu::PipelineCompilationOptions { constants, ..Default::default() };
        let make = |blend: Blend, kind: BatchKind| {
            let (vs, fs, topology, buffers) = {
                (
                    if kind == BatchKind::Bitmaps { "vs_bitmap" } else { "vs_main" },
                    if kind == BatchKind::Bitmaps { "fs_bitmap" } else { "fs_main" },
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
                    BatchKind::Bitmaps => "bitmaps",
                    BatchKind::Sprites => "sprites",
                }),
                layout: Some(&pipeline_layout),
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
        let sprite_pipelines = [make(Blend::Normal, BatchKind::Sprites), make(Blend::Additive, BatchKind::Sprites)];
        let bitmap_pipelines = [make(Blend::Normal, BatchKind::Bitmaps), make(Blend::Additive, BatchKind::Bitmaps)];
        let sprites = art
            .sprites
            .iter()
            .map(|s| SpriteInfo {
                bounds: s.bounds,
                first_layer: s.first_layer,
                layer_count: s.layer_count,
                compose: s.compose,
                bitmap: (s.flags & wf_art::sprite_flags::BITMAP != 0)
                    .then(|| [s.bitmap[0] & 0xffff, s.bitmap[0] >> 16, s.bitmap[1] & 0xffff, s.bitmap[1] >> 16]),
            })
            .collect();
        let band_count = |i: u32| {
            let o = i as usize * 8;
            art.bands.data.get(o..o + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        let band_texels = (art.bands.data.len() / 8) as u32;
        let costs = art.layers.iter().map(|l| layer_cost(l, band_texels, band_count)).collect();
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

    /// Builds the frame's dynamic shapes into the dynamic region, unless all
    /// of them are already there (their points did not change).
    fn build_dynamic(&mut self, list: &DrawList) {
        let mut wanted: Vec<(u64, Primitive, &[[f32; 2]], i32)> = Vec::new();
        for draw in &list.draws {
            if let Draw::Shape { primitive, points, transform, .. } = draw {
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

/// Adds an instance's estimated fragment work to `stats`: each area in
/// pixels, scaled by the part of its screen box inside the window.
#[allow(clippy::too_many_arguments)]
fn estimate(stats: &mut RenderStats, costs: &[LayerCost], m: &[[f32; 3]; 3], layout: &super::Layout, size: (u32, u32), bounds: [f32; 4], first: u32, count: u32) {
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
    stats.quad_px += area(bounds);
    for l in first..first + count {
        let Some(c) = costs.get(l as usize) else { continue };
        let px = area(c.bbox);
        stats.layer_px += px;
        stats.curve_px += px * c.curves as f64;
        if c.noise {
            stats.noise_px += px;
        }
    }
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
        self.instances.clear();
        self.batches.clear();
        self.build_dynamic(list);
        self.stats.dynamic_us = t0.elapsed().as_secs_f64() * 1e6;
        for draw in &list.draws {
            match draw {
                Draw::Sprite { sprite, transform, color, blend, clip, pass, .. } => {
                    let Some(info) = self.sprites.get(sprite.0 as usize) else {
                        continue;
                    };
                    if let Some(rect) = info.bitmap {
                        let m = &transform.m;
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
                    if self.collect {
                        let before = self.stats.curve_px;
                        estimate(&mut self.stats, &self.costs, m, &layout, size, bounds, info.first_layer, info.layer_count);
                        let e = self.by_sprite.entry(sprite.0).or_default();
                        e.0 += 1;
                        e.1 += self.stats.curve_px - before;
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
                        let before = self.stats.curve_px;
                        estimate(&mut self.stats, &self.costs, m, &layout, size, [b[0] - d, b[1] - d, b[2] + d, b[3] + d], layer, 1);
                        let e = self.by_sprite.entry(u32::MAX).or_default();
                        e.0 += 1;
                        e.1 += self.stats.curve_px - before;
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
        // Logical -> clip over the whole window (the visible rect).
        let [x0, y0, w, h] = layout.visible;
        let view = ViewUniform {
            scale: [2.0 / w, -2.0 / h],
            offset: [-1.0 - x0 * 2.0 / w, 1.0 + y0 * 2.0 / h],
            time: self.time,
            pad: [0.0; 3],
        };
        self.queue.write_buffer(&self.view_buffer, 0, bytemuck::bytes_of(&view));
        let fading = list.fade.0 >> 24 != 0 && !self.counting;
        if fading {
            self.fade.prepare(&self.queue, list.fade.to_f32());
        }
        self.stats.instances = self.instances.len() as u32;
        self.stats.upload_bytes += (self.instances.len() * std::mem::size_of::<Instance>() + std::mem::size_of::<ViewUniform>()) as u64;
        self.stats.upload_us = t1.elapsed().as_secs_f64() * 1e6;
        let t2 = std::time::Instant::now();

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
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
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            let [ax, ay, aw, ah] = if list.menu { [0, 0, size.0, size.1] } else { layout.arena_px };
            let instances = self.instance_buffer.slice(ring_offset..);
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
                }
            }
            if fading {
                self.fade.draw(&mut pass, size);
            }
        }
        self.queue.submit([encoder.finish()]);
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
    /// rays, summed (the measured fragment work).
    pub fn count_visits(&mut self, list: &DrawList, size: (u32, u32)) -> f64 {
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
        let mut sum = 0.0;
        for y in 0..height {
            for x in 0..width {
                let o = (y * row + x * 8) as usize;
                sum += f16_to_f32(u16::from_le_bytes([data[o], data[o + 1]])) as f64 * 64.0;
            }
        }
        sum
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
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .map_err(|e| format!("cannot create the GPU device: {e}"))?;
    Ok((adapter, device, queue))
}
