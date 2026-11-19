//! The wgpu backend of the graphics seam: uploads the cooked art's GPU half
//! once, then turns each [`DrawList`] into Slug instance draws and shape
//! triangles, in order.

use super::{Blend, Draw, DrawList, Primitive, SCREEN_HEIGHT, SCREEN_WIDTH};
use wf_art::Pack;
use wgpu::util::DeviceExt;

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

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ShapeVertex {
    pos: [f32; 2],
    color: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BatchKind {
    Sprites,
    Bitmaps,
    Shapes,
}

struct Batch {
    kind: BatchKind,
    blend: Blend,
    start: u32,
    end: u32,
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
    shape_pipelines: [wgpu::RenderPipeline; 2],
    bitmap_pipelines: [wgpu::RenderPipeline; 2],
    sprites: Vec<SpriteInfo>,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    shape_buffer: wgpu::Buffer,
    shape_capacity: usize,
    // Scratch, reused every frame.
    instances: Vec<Instance>,
    shape_vertices: Vec<ShapeVertex>,
    batches: Vec<Batch>,
    /// The multisampled colour target and its size, resolved into the frame.
    msaa: Option<(wgpu::TextureView, (u32, u32))>,
    pub time: f32,
}

/// Samples a pixel. Slug sprites antialias themselves (one shading per pixel);
/// the multisampling is what antialiases the untextured shapes (lines, the
/// option hexagons, bars) and the hard edges of texel regions.
const SAMPLES: u32 = 4;

fn upload(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, image: &wf_art::Image, format: wgpu::TextureFormat) -> wgpu::TextureView {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: image.width, height: image.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        image.data,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn blend_state(blend: Blend) -> wgpu::BlendState {
    // The shaders output premultiplied colour.
    let dst = match blend {
        Blend::Normal => wgpu::BlendFactor::OneMinusSrcAlpha,
        Blend::Additive => wgpu::BlendFactor::One,
    };
    let c = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: dst, operation: wgpu::BlendOperation::Add };
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
        let curves = upload(&device, &queue, "slug curves", &art.curves, wgpu::TextureFormat::Rgba32Float);
        let bands = upload(&device, &queue, "slug bands", &art.bands, wgpu::TextureFormat::Rg32Uint);
        let layers = upload(&device, &queue, "slug layers", &art.layer_image, wgpu::TextureFormat::Rgba32Uint);
        let ramps = upload(&device, &queue, "ramps", &art.ramps, wgpu::TextureFormat::Rgba8Unorm);
        let bitmaps = upload(&device, &queue, "bitmaps", &art.bitmaps, wgpu::TextureFormat::Rgba8Unorm);
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
            ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("slug"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
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
        let instance_attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x2, 3 => Float32x4, 4 => Uint32x4];
        let shape_attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];
        let make = |blend: Blend, kind: BatchKind| {
            let shapes = kind == BatchKind::Shapes;
            let (vs, fs, topology, buffers) = if shapes {
                (
                    "vs_shape",
                    "fs_shape",
                    wgpu::PrimitiveTopology::TriangleList,
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<ShapeVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &shape_attrs,
                    },
                )
            } else {
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
                    BatchKind::Shapes => "shapes",
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
                multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend_state(blend)),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sprite_pipelines = [make(Blend::Normal, BatchKind::Sprites), make(Blend::Additive, BatchKind::Sprites)];
        let bitmap_pipelines = [make(Blend::Normal, BatchKind::Bitmaps), make(Blend::Additive, BatchKind::Bitmaps)];
        let shape_pipelines = [make(Blend::Normal, BatchKind::Shapes), make(Blend::Additive, BatchKind::Shapes)];
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
        let instance_capacity = 1024;
        let shape_capacity = 4096;
        let instance_buffer = Self::buffer(&device, "instances", instance_capacity * std::mem::size_of::<Instance>());
        let shape_buffer = Self::buffer(&device, "shapes", shape_capacity * std::mem::size_of::<ShapeVertex>());
        Renderer {
            device,
            queue,
            format,
            view_buffer,
            bind_group,
            sprite_pipelines,
            shape_pipelines,
            bitmap_pipelines,
            sprites,
            instance_buffer,
            instance_capacity,
            shape_buffer,
            shape_capacity,
            instances: Vec::new(),
            shape_vertices: Vec::new(),
            batches: Vec::new(),
            msaa: None,
            time: 0.0,
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

    fn push_batch(&mut self, kind: BatchKind, blend: Blend, start: u32, end: u32) {
        if let Some(last) = self.batches.last_mut() {
            if last.kind == kind && last.blend == blend && last.end == start {
                last.end = end;
                return;
            }
        }
        self.batches.push(Batch { kind, blend, start, end });
    }

    fn push_shape(&mut self, primitive: Primitive, points: &[[f32; 2]], colors: &[super::Argb], blend: Blend) {
        let start = self.shape_vertices.len() as u32;
        let v = |i: usize| ShapeVertex { pos: points[i], color: colors[i].to_f32() };
        let n = points.len();
        let tri = |out: &mut Vec<ShapeVertex>, a: usize, b: usize, c: usize| {
            out.push(v(a));
            out.push(v(b));
            out.push(v(c));
        };
        let line = |out: &mut Vec<ShapeVertex>, a: usize, b: usize| {
            // A line one logical pixel wide, like the original's 1 px lines
            // at 640x480.
            let (pa, pb) = (points[a], points[b]);
            let d = [pb[0] - pa[0], pb[1] - pa[1]];
            let len = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-6);
            let nrm = [-d[1] / len * 0.5, d[0] / len * 0.5];
            let (ca, cb) = (colors[a].to_f32(), colors[b].to_f32());
            let q = [
                ShapeVertex { pos: [pa[0] + nrm[0], pa[1] + nrm[1]], color: ca },
                ShapeVertex { pos: [pa[0] - nrm[0], pa[1] - nrm[1]], color: ca },
                ShapeVertex { pos: [pb[0] + nrm[0], pb[1] + nrm[1]], color: cb },
                ShapeVertex { pos: [pb[0] - nrm[0], pb[1] - nrm[1]], color: cb },
            ];
            out.extend_from_slice(&[q[0], q[1], q[2], q[2], q[1], q[3]]);
        };
        let out = &mut self.shape_vertices;
        match primitive {
            Primitive::TriangleStrip => (0..n.saturating_sub(2)).for_each(|i| tri(out, i, i + 1, i + 2)),
            Primitive::TriangleFan => (1..n.saturating_sub(1)).for_each(|i| tri(out, 0, i, i + 1)),
            Primitive::TriangleList => (0..n / 3).for_each(|i| tri(out, 3 * i, 3 * i + 1, 3 * i + 2)),
            Primitive::LineStrip => (0..n.saturating_sub(1)).for_each(|i| line(out, i, i + 1)),
            Primitive::LineList => (0..n / 2).for_each(|i| line(out, 2 * i, 2 * i + 1)),
        }
        let end = self.shape_vertices.len() as u32;
        if end > start {
            self.push_batch(BatchKind::Shapes, blend, start, end);
        }
    }

    /// Renders `list` into `target` (of `size` pixels), letterboxing the
    /// 4:3 screen.
    pub fn render(&mut self, list: &DrawList, target: &wgpu::TextureView, size: (u32, u32)) {
        let k = (size.0 as f32 / SCREEN_WIDTH).min(size.1 as f32 / SCREEN_HEIGHT).max(1e-3);
        let vw = SCREEN_WIDTH * k;
        let vh = SCREEN_HEIGHT * k;
        let vx = ((size.0 as f32 - vw) * 0.5).floor();
        let vy = ((size.1 as f32 - vh) * 0.5).floor();

        self.instances.clear();
        self.shape_vertices.clear();
        self.batches.clear();
        for draw in &list.draws {
            match draw {
                Draw::Sprite { sprite, transform, color, blend, clip } => {
                    let Some(info) = self.sprites.get(sprite.0 as usize) else { continue };
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
                        self.push_batch(BatchKind::Bitmaps, *blend, i, i + 1);
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
                    let i = self.instances.len() as u32;
                    self.instances.push(Instance {
                        bounds,
                        m0: [m[0][0], m[0][1], m[1][0], m[1][1]],
                        m1: [m[2][0], m[2][1]],
                        tint: color.to_f32(),
                        layers: [info.first_layer, info.layer_count, info.compose, 0],
                    });
                    self.push_batch(BatchKind::Sprites, *blend, i, i + 1);
                }
                Draw::Shape { primitive, points, colors, blend } => {
                    self.push_shape(*primitive, points, colors, *blend);
                }
            }
        }

        if self.instances.len() > self.instance_capacity {
            self.instance_capacity = self.instances.len().next_power_of_two();
            self.instance_buffer = Self::buffer(&self.device, "instances", self.instance_capacity * std::mem::size_of::<Instance>());
        }
        if self.shape_vertices.len() > self.shape_capacity {
            self.shape_capacity = self.shape_vertices.len().next_power_of_two();
            self.shape_buffer = Self::buffer(&self.device, "shapes", self.shape_capacity * std::mem::size_of::<ShapeVertex>());
        }
        self.queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&self.instances));
        self.queue.write_buffer(&self.shape_buffer, 0, bytemuck::cast_slice(&self.shape_vertices));
        let view = ViewUniform {
            scale: [2.0 / SCREEN_WIDTH, -2.0 / SCREEN_HEIGHT],
            offset: [-1.0, 1.0],
            time: self.time,
            pad: [0.0; 3],
        };
        self.queue.write_buffer(&self.view_buffer, 0, bytemuck::bytes_of(&view));

        if self.msaa.as_ref().is_none_or(|m| m.1 != size) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("msaa"),
                size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: SAMPLES,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.msaa = Some((texture.create_view(&wgpu::TextureViewDescriptor::default()), size));
        }
        let msaa = &self.msaa.as_ref().expect("msaa target").0;
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let clear = list.clear.to_f32();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa,
                    depth_slice: None,
                    resolve_target: Some(target),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear[0] as f64,
                            g: clear[1] as f64,
                            b: clear[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(vx, vy, vw, vh, 0.0, 1.0);
            pass.set_bind_group(0, &self.bind_group, &[]);
            for batch in &self.batches {
                match batch.kind {
                    BatchKind::Sprites => {
                        pass.set_pipeline(&self.sprite_pipelines[blend_index(batch.blend)]);
                        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
                        pass.draw(0..4, batch.start..batch.end);
                    }
                    BatchKind::Bitmaps => {
                        pass.set_pipeline(&self.bitmap_pipelines[blend_index(batch.blend)]);
                        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
                        pass.draw(0..4, batch.start..batch.end);
                    }
                    BatchKind::Shapes => {
                        pass.set_pipeline(&self.shape_pipelines[blend_index(batch.blend)]);
                        pass.set_vertex_buffer(0, self.shape_buffer.slice(..));
                        pass.draw(batch.start..batch.end, 0..1);
                    }
                }
            }
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Renders `list` into a new `width` x `height` image and reads it back
    /// as RGBA8 (for tests and screenshots).
    pub fn render_to_rgba(&mut self, list: &DrawList, width: u32, height: u32) -> Vec<u8> {
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
        self.render(list, &view, (width, height));
        let row = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) },
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

/// Creates a device for `surface` (or a headless one when `None`).
pub fn create_device(instance: &wgpu::Instance, surface: Option<&wgpu::Surface>) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), String> {
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
