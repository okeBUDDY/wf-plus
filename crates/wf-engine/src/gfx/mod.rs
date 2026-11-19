//! The graphics seam. Game code describes a frame as a [`DrawList`] in the
//! original 640x480 screen space; a backend (`gpu`, wgpu) turns it into
//! pixels. Nothing above this module knows about the GPU.
//!
//! The draw calls mirror what the game asked of its renderer, not how
//! Direct3D did it:
//! * [`DrawList::sprite`] = `HLSPat::Draw` (a pattern under a transform,
//!   tinted, normal or additive), now drawn from cooked Slug art;
//! * [`DrawList::shape`] = `HL3D::DrawPoints` (untextured strips, fans,
//!   lines) whose points come from game logic. The constant shapes (see
//!   the game's shape table) are cooked sprites, drawn with [`DrawList::sprite`].
//!
//! Shapes are Slug layers too: filled triangles, and lines one screen pixel
//! wide whatever the transform, as Direct3D 7 drew them.

pub mod gpu;
pub mod layout;
pub mod scale;
pub mod transform;

pub use layout::Layout;

pub use wf_art::shape::Primitive;

pub use transform::Transform;

/// The logical screen of the original game.
pub const SCREEN_WIDTH: f32 = 640.0;
pub const SCREEN_HEIGHT: f32 = 480.0;

/// A colour in the D3DCOLOR layout the game uses: 0xAARRGGBB, straight alpha.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Argb(pub u32);

impl Argb {
    pub const WHITE: Argb = Argb(0xffff_ffff);

    pub fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Argb((a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32)
    }
    pub fn alpha(self) -> u8 {
        (self.0 >> 24) as u8
    }
    /// Straight RGBA in 0..1.
    pub fn to_f32(self) -> [f32; 4] {
        let c = self.0;
        [
            ((c >> 16) & 0xff) as f32 / 255.0,
            ((c >> 8) & 0xff) as f32 / 255.0,
            (c & 0xff) as f32 / 255.0,
            (c >> 24) as f32 / 255.0,
        ]
    }
}

/// How a draw lands on the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Blend {
    /// src * srcalpha + dst * (1 - srcalpha) (the game's blend 0).
    Normal,
    /// src * srcalpha + dst (the game's blend 1).
    Additive,
}

impl Blend {
    /// The game's `blend` argument.
    pub fn from_game(blend: i32) -> Self {
        if blend == 1 { Blend::Additive } else { Blend::Normal }
    }
}

/// A cooked art sprite, by index into the art pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpriteId(pub u32);

/// Which pass a draw belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pass {
    /// Behind the arena, and drawn into the margins around it (the grid).
    Background,
    /// Cut to the arena's rect: the game proper.
    #[default]
    Arena,
    /// In the list's order, over the whole view: the HUD, which keeps to
    /// the view's edges in a narrower arena.
    Overlay,
}

#[derive(Clone, Debug)]
pub enum Draw {
    /// `clip` (min x, min y, max x, max y in the sprite's own space) cuts
    /// the sprite to a rect with hard edges: a texel region of a sheet.
    Sprite { sprite: SpriteId, transform: Transform, color: Argb, blend: Blend, clip: Option<[f32; 4]>, key: u32, pass: Pass },
    /// A shape whose local points come from game logic, under `transform`.
    Shape { primitive: Primitive, points: Vec<[f32; 2]>, transform: Transform, color: Argb, blend: Blend, key: u32, pass: Pass },
}

impl Draw {
    pub fn key(&self) -> u32 {
        match self {
            Draw::Sprite { key, .. } | Draw::Shape { key, .. } => *key,
        }
    }
    pub fn pass(&self) -> Pass {
        match self {
            Draw::Sprite { pass, .. } | Draw::Shape { pass, .. } => *pass,
        }
    }
    pub fn transform(&self) -> &Transform {
        match self {
            Draw::Sprite { transform, .. } | Draw::Shape { transform, .. } => transform,
        }
    }
    fn transform_mut(&mut self) -> &mut Transform {
        match self {
            Draw::Sprite { transform, .. } | Draw::Shape { transform, .. } => transform,
        }
    }
}

/// One frame's worth of drawing, in order: the render snapshot the game
/// hands the renderer after each tick.
#[derive(Clone, Default)]
pub struct DrawList {
    pub clear: Argb,
    pub draws: Vec<Draw>,
    /// The arena (logical units) the game ran in: 640x480 for Classic.
    pub arena: [f32; 2],
    /// The interpolation key of the draws being added (0: none) and the
    /// index of the next draw under it.
    key: u32,
    sub: u32,
    pass: Pass,
    /// Added to every transform's translation (a HUD block anchored to
    /// another edge of a wider arena).
    origin: [f32; 2],
}

impl DrawList {
    pub fn new() -> Self {
        DrawList { clear: Argb(0xff00_0000), arena: [SCREEN_WIDTH, SCREEN_HEIGHT], ..Default::default() }
    }

    pub fn reset(&mut self) {
        self.clear = Argb(0xff00_0000);
        self.draws.clear();
        self.key = 0;
        self.sub = 0;
        self.pass = Pass::Arena;
        self.origin = [0.0; 2];
    }

    /// Moves the draws that follow by `origin` (0, 0 to stop).
    pub fn set_origin(&mut self, origin: [f32; 2]) {
        self.origin = origin;
    }

    fn place(&self, t: &Transform) -> Transform {
        let mut t = *t;
        if self.origin != [0.0; 2] {
            t.m[2][0] += self.origin[0];
            t.m[2][1] += self.origin[1];
        }
        t
    }

    /// The draws that follow belong to object `key` (non-zero, stable
    /// across ticks, e.g. an actor's slot), for interpolation; 0 ends it.
    pub fn set_key(&mut self, key: u32) {
        self.key = key;
        self.sub = 0;
    }

    /// The pass of the draws that follow.
    pub fn set_pass(&mut self, pass: Pass) {
        self.pass = pass;
    }

    fn next_key(&mut self) -> u32 {
        if self.key == 0 {
            return 0;
        }
        let k = self.key.wrapping_mul(64).wrapping_add(self.sub.min(63));
        self.sub += 1;
        k
    }

    /// Draws `sprite` under `transform`, tinted by `color`. Like
    /// `HLSPat::Draw`, a fully transparent tint draws nothing.
    pub fn sprite(&mut self, sprite: SpriteId, transform: &Transform, color: Argb, blend: Blend) {
        if color.alpha() == 0 {
            return;
        }
        let (key, pass, transform) = (self.next_key(), self.pass, self.place(transform));
        self.draws.push(Draw::Sprite { sprite, transform, color, blend, clip: None, key, pass });
    }

    /// Draws the part of `sprite` inside `clip` (in the sprite's space).
    pub fn sprite_clipped(&mut self, sprite: SpriteId, transform: &Transform, clip: [f32; 4], color: Argb, blend: Blend) {
        if color.alpha() == 0 || clip[0] >= clip[2] || clip[1] >= clip[3] {
            return;
        }
        let (key, pass, transform) = (self.next_key(), self.pass, self.place(transform));
        self.draws.push(Draw::Sprite { sprite, transform, color, blend, clip: Some(clip), key, pass });
    }

    /// Draws `points` (in the shape's own space) under `transform` in one
    /// colour. The backend rebuilds the shape's curves only when its points
    /// change; the transform is applied on the GPU.
    pub fn shape(&mut self, primitive: Primitive, transform: &Transform, points: &[[f32; 2]], color: Argb, blend: Blend) {
        if color.alpha() == 0 {
            return;
        }
        let (key, pass, transform) = (self.next_key(), self.pass, self.place(transform));
        self.draws.push(Draw::Shape { primitive, points: points.to_vec(), transform, color, blend, key, pass });
    }
}

/// How far (logical units) a keyed draw may move in one tick and still be
/// interpolated; further is a teleport or a wrap, and snaps.
pub const SNAP_DISTANCE: f32 = 48.0;

/// The snapshot to show `alpha` (0..1) of the way from `prev` to `cur`:
/// `cur`, with each keyed draw's transform blended from the draw of the
/// same key in `prev`. Only for display; the game's state is untouched.
/// Draws snap (show `cur`) across teleports, wraps, flips and sprite
/// changes.
pub fn interpolate(prev: &DrawList, cur: &DrawList, alpha: f32, out: &mut DrawList) {
    out.clone_from(cur);
    if alpha >= 1.0 || prev.draws.is_empty() {
        return;
    }
    let mut before = std::collections::HashMap::with_capacity(prev.draws.len());
    for d in &prev.draws {
        if d.key() != 0 {
            before.insert(d.key(), d);
        }
    }
    let a = alpha.max(0.0);
    for d in &mut out.draws {
        let k = d.key();
        if k == 0 {
            continue;
        }
        let Some(p) = before.get(&k) else { continue };
        let same = match (&**p, &*d) {
            (Draw::Sprite { sprite: s0, .. }, Draw::Sprite { sprite: s1, .. }) => s0 == s1,
            (Draw::Shape { primitive: p0, .. }, Draw::Shape { primitive: p1, .. }) => p0 == p1,
            _ => false,
        };
        if !same {
            continue;
        }
        let (m0, m1) = (p.transform().m, d.transform().m);
        let (dx, dy) = (m1[2][0] - m0[2][0], m1[2][1] - m0[2][1]);
        if dx * dx + dy * dy > SNAP_DISTANCE * SNAP_DISTANCE {
            continue;
        }
        // A flip or a turn of more than ~45 degrees snaps too.
        let det0 = m0[0][0] * m0[1][1] - m0[0][1] * m0[1][0];
        let det1 = m1[0][0] * m1[1][1] - m1[0][1] * m1[1][0];
        let dot = m0[0][0] * m1[0][0] + m0[0][1] * m1[0][1];
        let n = ((m0[0][0] * m0[0][0] + m0[0][1] * m0[0][1]) * (m1[0][0] * m1[0][0] + m1[0][1] * m1[0][1])).sqrt();
        if det0 * det1 <= 0.0 || dot < n * 0.7 {
            continue;
        }
        let t = d.transform_mut();
        for r in 0..3 {
            for c in 0..2 {
                t.m[r][c] = m0[r][c] + (m1[r][c] - m0[r][c]) * a;
            }
        }
    }
}
