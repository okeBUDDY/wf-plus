//! Render scale: the frame is drawn into a target of the window's size
//! times the scale, then stretched onto the window (linear filtering).
//! At scale 1 the frame goes straight to the window. Above 2x the blit
//! averages several taps (each a bilinear 2x2), so supersampling does not
//! skip texels.

use std::sync::atomic::{AtomicU32, Ordering};

use super::DrawList;
use super::gpu::Renderer;

const BLIT: &str = r"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    // One triangle over the whole target.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    // The window pixel's footprint, in uv and in source texels.
    let px = vec2<f32>(abs(dpdx(i.uv).x), abs(dpdy(i.uv).y));
    let texels = px * vec2<f32>(textureDimensions(src));
    // Taps no more than 2 texels apart, each averaging 2x2 texels.
    let n = vec2<i32>(clamp(ceil(texels * 0.5), vec2<f32>(1.0), vec2<f32>(4.0)));
    var sum = vec4<f32>(0.0);
    for (var y = 0; y < n.y; y++) {
        for (var x = 0; x < n.x; x++) {
            let o = (vec2<f32>(f32(x), f32(y)) + 0.5) / vec2<f32>(n) - 0.5;
            sum += textureSampleLevel(src, smp, i.uv + o * px, 0.0);
        }
    }
    return sum / f32(n.x * n.y);
}
";

pub struct Scaler {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The scaled target, its size, and the bind group reading it.
    target: Option<(wgpu::TextureView, (u32, u32), wgpu::BindGroup)>,
}

/// The GPU's largest texture side (Scaler::new), at most 8192.
static MAX_SIDE: AtomicU32 = AtomicU32::new(8192);

/// The scaled target's size for a window of `size` at `scale`: scaled
/// down as a whole where a side would pass the GPU's limit.
pub fn scaled_size(size: (u32, u32), scale: f32) -> (u32, u32) {
    let max = MAX_SIDE.load(Ordering::Relaxed) as f32;
    let scale = scale.min(max / size.0.max(1) as f32).min(max / size.1.max(1) as f32);
    let s = |v: u32| ((v as f32 * scale).round() as u32).clamp(1, max as u32);
    (s(size.0), s(size.1))
}

impl Scaler {
    pub fn new(r: &Renderer) -> Self {
        let device = r.device();
        MAX_SIDE.store(device.limits().max_texture_dimension_2d.min(8192), Ordering::Relaxed);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit"),
            source: wgpu::ShaderSource::Wgsl(BLIT.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: r.format(),
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Scaler { pipeline, layout, sampler, target: None }
    }

    /// Draws `list` for a window of `size` into `out` at `scale`.
    pub fn render(&mut self, r: &mut Renderer, list: &DrawList, out: &wgpu::TextureView, size: (u32, u32), scale: f32) {
        // Menus are always drawn at the window's resolution.
        if (scale - 1.0).abs() < 1e-3 || list.menu {
            self.target = None;
            r.render(list, out, size);
            return;
        }
        let scaled = scaled_size(size, scale);
        if self.target.as_ref().is_none_or(|(_, s, _)| *s != scaled) {
            let device = r.device();
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scaled"),
                size: wgpu::Extent3d { width: scaled.0, height: scaled.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: r.format(),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("blit"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            self.target = Some((view, scaled, bind));
        }
        let (view, _, bind) = self.target.as_ref().expect("scaled target");
        r.render_for(list, view, scaled, size);
        let mut encoder = r.device().create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("blit") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: out,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
        r.queue().submit([encoder.finish()]);
    }
}
