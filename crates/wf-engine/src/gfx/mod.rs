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
pub mod transform;

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

#[derive(Clone, Debug)]
pub enum Draw {
    /// `clip` (min x, min y, max x, max y in the sprite's own space) cuts
    /// the sprite to a rect with hard edges: a texel region of a sheet.
    Sprite { sprite: SpriteId, transform: Transform, color: Argb, blend: Blend, clip: Option<[f32; 4]> },
    /// A shape whose local points come from game logic, under `transform`.
    Shape { primitive: Primitive, points: Vec<[f32; 2]>, transform: Transform, color: Argb, blend: Blend },
}

/// One frame's worth of drawing, in order.
#[derive(Default)]
pub struct DrawList {
    pub clear: Argb,
    pub draws: Vec<Draw>,
}

impl DrawList {
    pub fn new() -> Self {
        DrawList { clear: Argb(0xff00_0000), draws: Vec::new() }
    }

    pub fn reset(&mut self) {
        self.clear = Argb(0xff00_0000);
        self.draws.clear();
    }

    /// Draws `sprite` under `transform`, tinted by `color`. Like
    /// `HLSPat::Draw`, a fully transparent tint draws nothing.
    pub fn sprite(&mut self, sprite: SpriteId, transform: &Transform, color: Argb, blend: Blend) {
        if color.alpha() == 0 {
            return;
        }
        self.draws.push(Draw::Sprite { sprite, transform: *transform, color, blend, clip: None });
    }

    /// Draws the part of `sprite` inside `clip` (in the sprite's space).
    pub fn sprite_clipped(&mut self, sprite: SpriteId, transform: &Transform, clip: [f32; 4], color: Argb, blend: Blend) {
        if color.alpha() == 0 || clip[0] >= clip[2] || clip[1] >= clip[3] {
            return;
        }
        self.draws.push(Draw::Sprite { sprite, transform: *transform, color, blend, clip: Some(clip) });
    }

    /// Draws `points` (in the shape's own space) under `transform` in one
    /// colour. The backend rebuilds the shape's curves only when its points
    /// change; the transform is applied on the GPU.
    pub fn shape(&mut self, primitive: Primitive, transform: &Transform, points: &[[f32; 2]], color: Argb, blend: Blend) {
        if color.alpha() == 0 {
            return;
        }
        self.draws.push(Draw::Shape { primitive, points: points.to_vec(), transform: *transform, color, blend });
    }
}
