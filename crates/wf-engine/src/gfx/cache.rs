//! The sprite cache: a sprite's whole Slug layer stack rasterized once per
//! scale bucket into an atlas page, then drawn as a plain textured quad
//! (premultiplied, linear filtering, the transform and the tint applied as
//! for the Slug path). The heavy sprites (the explosions' eight nested
//! layers, the bullets) cost one texel fetch a pixel instead of hundreds of
//! curve visits.
//!
//! * A bucket is a 1.25x step of scale (device pixels per local unit): an
//!   instance uses the nearest bucket at or above its largest scale, so the
//!   atlas is always at least as fine as the screen and filtering only
//!   shrinks it (edges keep their sharpness while an explosion zooms).
//! * The cached paint is a function of the sprite's own coordinates only
//!   (solid colours, gradients and noise in the sprite's space; the shader
//!   reads no time), and the instance tint multiplies the result, so one
//!   entry serves every instance. Strokes are not cached: their width is a
//!   screen pixel whatever the scale.
//! * Entries are rasterized lazily, in the frame that first needs them,
//!   within a per-frame budget (the rest draw with Slug until their turn),
//!   so nothing stalls start-up or a frame.
//! * Pages are allocated as needed, up to [`MAX_BYTES`]; when they are full
//!   the least recently used page not drawn from this frame is emptied.

use std::collections::HashMap;

/// Atlas page size (texels): within every mobile GPU's limits.
pub const PAGE_SIZE: u32 = 2048;
/// The memory budget of all pages (RGBA8).
pub const MAX_BYTES: u64 = 64 << 20;
/// Transparent texels around an entry's sprite bounds (the coverage's
/// antialiasing reaches about one texel past the outline; the quad drawn
/// stops one texel short of the entry's edge, so the filter never reads a
/// neighbour).
pub const PAD: u32 = 3;
/// The bucket step.
pub const STEP: f32 = 1.25;
/// Atlas texels per device pixel, at least: an entry is rasterized at the
/// bucket at or above twice the instance's scale, and drawn with a
/// four-tap one-pixel box filter. Measured on every cached sprite
/// (`sprite_cache_matches_slug`, mean difference per sprite): one tap at
/// 1x was visibly soft on thin lines (0.17 mean over the sprites), one tap
/// at 1.5x 0.17 (the title art's lettering up to 0.77), four taps at 2x
/// 0.12 (the lettering under 0.47); 2.5x and 3x are no better. An entry
/// over half a page takes a lower bucket, down to the screen's scale.
pub const OVERSAMPLE: f32 = 2.0;
/// Shelf heights are multiples of this.
const SHELF_ROUND: u32 = 8;

/// The bucket of an instance whose largest scale is `scale` device pixels
/// per local unit: the smallest `n` with `STEP^n >= scale` (within 0.1%).
pub fn bucket(scale: f32) -> i32 {
    (scale.ln() / STEP.ln() - 1e-3).ceil().clamp(-64.0, 64.0) as i32
}

pub fn bucket_scale(n: i32) -> f32 {
    STEP.powi(n)
}

/// The largest singular value of the 2x2 linear part `m` (its largest
/// stretch in any direction).
pub fn max_scale(m: [f32; 4]) -> f32 {
    let p = m.iter().map(|v| v * v).sum::<f32>();
    let det = m[0] * m[3] - m[1] * m[2];
    ((p + (p * p - 4.0 * det * det).max(0.0).sqrt()) * 0.5).sqrt()
}

/// A cached sprite at one bucket.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub page: usize,
    /// Texel rect in the page.
    pub rect: [u32; 4],
    /// The local coordinates of the rect's top-left texel corner.
    pub origin: [f32; 2],
    /// Texels per local unit.
    pub scale: f32,
    /// Rows rasterized so far (from the top): a large entry is filled a
    /// strip at a time over a few frames, and drawn from once complete.
    pub done: u32,
}

impl Entry {
    /// The local quad drawn from the entry, one texel inside its rect, and
    /// that quad's texel rect.
    pub fn quad(&self) -> ([f32; 4], [u32; 4]) {
        let [x, y, w, h] = self.rect;
        let (o, s) = (self.origin, self.scale);
        ([o[0] + 1.0 / s, o[1] + 1.0 / s, o[0] + (w - 1) as f32 / s, o[1] + (h - 1) as f32 / s], [x + 1, y + 1, w - 2, h - 2])
    }

    /// The page texel coordinates of local point `p`.
    pub fn texel_of(&self, p: [f32; 2]) -> [f32; 2] {
        [(p[0] - self.origin[0]) * self.scale + self.rect[0] as f32, (p[1] - self.origin[1]) * self.scale + self.rect[1] as f32]
    }

    /// The whole rect in local units (what the rasterization covers).
    pub fn local_rect(&self) -> [f32; 4] {
        self.strip(0, self.rect[3])
    }

    /// Rows `from..to` of the rect in local units.
    pub fn strip(&self, from: u32, to: u32) -> [f32; 4] {
        let w = self.rect[2];
        let (o, s) = (self.origin, self.scale);
        [o[0], o[1] + from as f32 / s, o[0] + w as f32 / s, o[1] + to as f32 / s]
    }

    pub fn complete(&self) -> bool {
        self.done >= self.rect[3]
    }
}

/// The entry's size for sprite bounds `b` at `scale`.
pub fn entry_size(b: [f32; 4], scale: f32) -> (u32, u32) {
    let w = ((b[2] - b[0]).max(0.0) * scale).ceil() as u32 + 2 * PAD;
    let h = ((b[3] - b[1]).max(0.0) * scale).ceil() as u32 + 2 * PAD;
    (w, h)
}

struct Shelf {
    y: u32,
    h: u32,
    x: u32,
}

/// One page's free space: shelves of rounded heights, filled left to right.
#[derive(Default)]
pub struct Shelves {
    shelves: Vec<Shelf>,
    top: u32,
}

impl Shelves {
    pub fn alloc(&mut self, w: u32, h: u32, size: u32) -> Option<(u32, u32)> {
        if w > size || h > size {
            return None;
        }
        let hr = h.div_ceil(SHELF_ROUND) * SHELF_ROUND;
        // The lowest shelf that takes it without wasting more than a
        // quarter of its height.
        let best = self
            .shelves
            .iter_mut()
            .filter(|s| s.h >= hr && s.h <= hr + hr / 4 + SHELF_ROUND && size - s.x >= w)
            .min_by_key(|s| s.h);
        if let Some(s) = best {
            let at = (s.x, s.y);
            s.x += w;
            return Some(at);
        }
        if self.top + hr > size {
            return None;
        }
        let at = (0, self.top);
        self.shelves.push(Shelf { y: self.top, h: hr, x: w });
        self.top += hr;
        Some(at)
    }

    pub fn clear(&mut self) {
        self.shelves.clear();
        self.top = 0;
    }
}

pub struct Page {
    pub view: wgpu::TextureView,
    pub bind_group: wgpu::BindGroup,
    shelves: Shelves,
    /// The frame it was last drawn from or written to.
    last_used: u64,
    keys: Vec<(u32, i32)>,
}

/// What the cache did this frame (for the perf bench).
#[derive(Clone, Copy, Debug, Default)]
pub struct CacheStats {
    /// Draws from a complete entry.
    pub hits: u32,
    /// Strips rasterized (a whole entry or part of one).
    pub rasterized: u32,
    /// Draws that wanted an entry and drew with Slug instead (over the
    /// frame's budget, too large, or the atlas full).
    pub deferred: u32,
    pub evicted_pages: u32,
    pub pages: u32,
    pub entries: u32,
}

pub struct SpriteCache {
    pub enabled: bool,
    device: wgpu::Device,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    page_size: u32,
    max_pages: usize,
    pub pages: Vec<Page>,
    entries: HashMap<(u32, i32), Entry>,
    frame: u64,
    pub stats: CacheStats,
}

impl SpriteCache {
    pub fn new(device: &wgpu::Device, layout: wgpu::BindGroupLayout) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sprite cache"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let page_size = PAGE_SIZE.min(device.limits().max_texture_dimension_2d);
        let page_bytes = page_size as u64 * page_size as u64 * 4;
        SpriteCache {
            enabled: true,
            device: device.clone(),
            layout,
            sampler,
            page_size,
            max_pages: (MAX_BYTES / page_bytes).max(1) as usize,
            pages: Vec::new(),
            entries: HashMap::new(),
            frame: 0,
            stats: CacheStats::default(),
        }
    }

    pub fn page_size(&self) -> u32 {
        self.page_size
    }

    /// The largest entry side: half a page.
    pub fn max_entry(&self) -> u32 {
        self.page_size / 2
    }

    pub fn begin_frame(&mut self) {
        self.frame += 1;
        self.stats = CacheStats { pages: self.pages.len() as u32, entries: self.entries.len() as u32, ..Default::default() };
    }

    /// Empties the cache (the pages are kept).
    pub fn clear(&mut self) {
        self.entries.clear();
        for p in &mut self.pages {
            p.shelves.clear();
            p.keys.clear();
        }
    }

    /// The entry of `sprite` at bucket `n`, marking its page used.
    pub fn get(&mut self, sprite: u32, n: i32) -> Option<Entry> {
        let e = *self.entries.get(&(sprite, n))?;
        self.pages[e.page].last_used = self.frame;
        Some(e)
    }

    /// Records the rows of an entry rasterized so far.
    pub fn set_done(&mut self, sprite: u32, n: i32, done: u32) {
        if let Some(e) = self.entries.get_mut(&(sprite, n)) {
            e.done = done;
        }
    }

    /// Room for `sprite` at bucket `n` with bounds `b`: a new page, or the
    /// least recently used one emptied, when none has room. The caller
    /// rasterizes it this frame, before it is drawn from.
    pub fn insert(&mut self, sprite: u32, n: i32, b: [f32; 4]) -> Option<Entry> {
        let scale = bucket_scale(n);
        let (w, h) = entry_size(b, scale);
        if w > self.max_entry() || h > self.max_entry() {
            return None;
        }
        let size = self.page_size;
        let mut found = self.pages.iter_mut().enumerate().find_map(|(i, p)| p.shelves.alloc(w, h, size).map(|at| (i, at)));
        if found.is_none() && self.pages.len() < self.max_pages {
            self.pages.push(self.new_page());
            let i = self.pages.len() - 1;
            found = self.pages[i].shelves.alloc(w, h, size).map(|at| (i, at));
        }
        if found.is_none() {
            // Evict the page drawn from least recently, never one this
            // frame draws from (its texels must hold until the frame ends).
            let frame = self.frame;
            let victim = self.pages.iter().enumerate().filter(|(_, p)| p.last_used < frame).min_by_key(|(_, p)| p.last_used).map(|(i, _)| i)?;
            let p = &mut self.pages[victim];
            for k in p.keys.drain(..) {
                self.entries.remove(&k);
            }
            p.shelves.clear();
            self.stats.evicted_pages += 1;
            found = p.shelves.alloc(w, h, size).map(|at| (victim, at));
        }
        let (page, (x, y)) = found?;
        let e = Entry { page, rect: [x, y, w, h], origin: [b[0] - PAD as f32 / scale, b[1] - PAD as f32 / scale], scale, done: 0 };
        let p = &mut self.pages[page];
        p.last_used = self.frame;
        p.keys.push((sprite, n));
        self.entries.insert((sprite, n), e);
        self.stats.pages = self.pages.len() as u32;
        self.stats.entries = self.entries.len() as u32;
        Some(e)
    }

    fn new_page(&self) -> Page {
        crate::log::breadcrumb(&format!(
            "sprite cache: page {} of at most {}, {}x{} {FORMAT:?} (device max 2D texture {})",
            self.pages.len() + 1,
            self.max_pages,
            self.page_size,
            self.page_size,
            self.device.limits().max_texture_dimension_2d
        ));
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite cache page"),
            size: wgpu::Extent3d { width: self.page_size, height: self.page_size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprite cache page"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        Page { view, bind_group, shelves: Shelves::default(), last_used: 0, keys: Vec::new() }
    }

    /// Bytes of atlas memory allocated.
    pub fn bytes(&self) -> u64 {
        self.pages.len() as u64 * self.page_size as u64 * self.page_size as u64 * 4
    }
}

/// The pages' format: premultiplied RGBA8, as the Slug path's output.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The bind group layout of a page (group 1 of the cached pipelines).
pub fn page_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("sprite cache page"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_are_at_or_above_the_scale() {
        for i in 1..2000 {
            let s = i as f32 * 0.01;
            let b = bucket_scale(bucket(s));
            assert!(b >= s * 0.999, "{s} -> {b}");
            assert!(b < s * STEP * 1.001, "{s} -> {b}");
        }
    }

    #[test]
    fn max_scale_is_the_largest_stretch() {
        assert!((max_scale([2.0, 0.0, 0.0, 3.0]) - 3.0).abs() < 1e-5);
        let (c, s) = (0.6f32, 0.8f32);
        assert!((max_scale([2.0 * c, 2.0 * s, -s, c]) - 2.0).abs() < 1e-5);
        assert!((max_scale([1.0, 0.0, 1.0, 1.0]) - 1.618034).abs() < 1e-4);
    }

    #[test]
    fn shelves_pack_without_overlap() {
        let mut s = Shelves::default();
        let mut rects = Vec::new();
        let mut i = 0u32;
        while let Some((x, y)) = s.alloc(20 + i * 7 % 90, 20 + i * 13 % 70, 512) {
            rects.push([x, y, 20 + i * 7 % 90, 20 + i * 13 % 70]);
            i += 1;
        }
        assert!(rects.len() > 20);
        for (a, ra) in rects.iter().enumerate() {
            assert!(ra[0] + ra[2] <= 512 && ra[1] + ra[3] <= 512);
            for rb in &rects[a + 1..] {
                let apart = ra[0] + ra[2] <= rb[0] || rb[0] + rb[2] <= ra[0] || ra[1] + ra[3] <= rb[1] || rb[1] + rb[3] <= ra[1];
                assert!(apart, "{ra:?} {rb:?}");
            }
        }
    }

    #[test]
    fn the_drawn_quad_maps_to_its_texels() {
        let e = Entry { page: 0, rect: [10, 20, 40, 30], origin: [-8.0, -6.0], scale: 2.0, done: 30 };
        let (q, t) = e.quad();
        // Local -> texel is linear: (l - origin) * scale + rect origin.
        assert_eq!(((q[0] + 8.0) * 2.0 + 10.0, (q[1] + 6.0) * 2.0 + 20.0), (t[0] as f32, t[1] as f32));
        assert_eq!(((q[2] + 8.0) * 2.0 + 10.0, (q[3] + 6.0) * 2.0 + 20.0), ((t[0] + t[2]) as f32, (t[1] + t[3]) as f32));
        assert_eq!(e.texel_of([q[0], q[1]]), [t[0] as f32, t[1] as f32]);
    }
}
