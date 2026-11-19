//! The graphics seam. Game code describes a frame as a [`DrawList`] in the
//! original 640x480 screen space; a backend (`gpu`, wgpu) turns it into
//! pixels. Nothing above this module knows about the GPU.
//!
//! The draw calls mirror what the game asked of its renderer, not how
//! Direct3D did it:
//! * [`DrawList::sprite`] = `HLSPat::Draw` (a pattern under a transform,
//!   tinted, normal or additive), now drawn from cooked Slug art;
//! * [`DrawList::shape`] = `HL3D::DrawPoints` (untextured strips, fans,
//!   lines);
//! * [`DrawList::fill_screen`] = the screen-fade `Sprite` overlay.

pub mod gpu;
pub mod transform;

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

/// The primitive of an untextured shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primitive {
    TriangleStrip,
    TriangleFan,
    TriangleList,
    LineStrip,
    LineList,
}

#[derive(Clone, Debug)]
pub enum Draw {
    Sprite { sprite: SpriteId, transform: Transform, color: Argb, blend: Blend },
    /// Points already transformed to screen space.
    Shape { primitive: Primitive, points: Vec<[f32; 2]>, colors: Vec<Argb>, blend: Blend },
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
        self.draws.push(Draw::Sprite { sprite, transform: *transform, color, blend });
    }

    /// Draws `points` (in the shape's own space) under `transform` in one
    /// colour.
    pub fn shape(&mut self, primitive: Primitive, transform: &Transform, points: &[[f32; 2]], color: Argb, blend: Blend) {
        let points: Vec<[f32; 2]> = points.iter().map(|p| transform.apply(p[0], p[1])).collect();
        let colors = vec![color; points.len()];
        self.draws.push(Draw::Shape { primitive, points, colors, blend });
    }

    /// Covers the screen in `color` (the screen fade).
    pub fn fill_screen(&mut self, color: Argb) {
        if color.alpha() == 0 {
            return;
        }
        let points = vec![[0.0, 0.0], [SCREEN_WIDTH, 0.0], [0.0, SCREEN_HEIGHT], [SCREEN_WIDTH, SCREEN_HEIGHT]];
        self.draws.push(Draw::Shape { primitive: Primitive::TriangleStrip, points, colors: vec![color; 4], blend: Blend::Normal });
    }
}
