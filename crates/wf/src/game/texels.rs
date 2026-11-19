//! DrawTexels: the HUD, title and options draw a few rects of their sheets
//! straight from the texture (no .pat record covers them), with the
//! texture wrapping horizontally. Here each sheet region is a cooked
//! sprite cut from the sheet at a known place; a texel rect becomes that
//! sprite clipped to the rect, once per horizontal wrap.

use super::Cx;
use crate::gfx::{Argb, Blend, SpriteId, Transform};

/// A sprite cut from a sheet.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub sprite: &'static str,
    /// Where the sprite's (0, 0) is on the sheet, in texels.
    pub origin: [f32; 2],
    /// The rows of the sheet it covers (the texture's v range).
    pub rows: [f32; 2],
    /// The sheet's width: texels wrap around it.
    pub wrap: f32,
}

/// The "WARNING :" band of etc0.png (cut at 0, 210, 256 x 44).
pub const ETC0_BAND: Region = Region { sprite: "etc0_extra00", origin: [0.0, 210.0], rows: [208.0, 256.0], wrap: 256.0 };
/// The weapon names of etc2.png (cut at 96, 129, 159 x 118).
pub const ETC2_WEAPONS: Region = Region { sprite: "etc2_extra00", origin: [96.0, 129.0], rows: [128.0, 248.0], wrap: 256.0 };

/// Caption strip `i` (0..3) of font2.png: 128 x 16 at (0, 16 i).
pub const fn font2_strip(i: usize) -> Region {
    const NAMES: [&str; 3] = ["font/strip/000", "font/strip/001", "font/strip/002"];
    let y = 16.0 * i as f32;
    Region { sprite: NAMES[i], origin: [0.0, y], rows: [y, y + 16.0], wrap: 128.0 }
}

/// HL3D::DrawTexels with D3DPT_TRIANGLESTRIP of an axis-aligned quad:
/// `quad` (top left, top right, bottom left, bottom right) in the space of
/// `transform`, showing the texels `texels` (same order) of `region`.
pub fn draw(cx: &mut Cx, region: &Region, transform: &Transform, quad: &[[f32; 2]; 4], texels: &[[i16; 2]; 4], color: u32) {
    let Some(sprite) = cx.art.find(region.sprite) else { return };
    draw_sprite(cx, sprite, region, transform, quad, texels, color);
}

fn draw_sprite(cx: &mut Cx, sprite: SpriteId, region: &Region, transform: &Transform, quad: &[[f32; 2]; 4], texels: &[[i16; 2]; 4], color: u32) {
    let (x0, y0, x1, y1) = (quad[0][0], quad[0][1], quad[3][0], quad[3][1]);
    let (u0, v0, u1, v1) = (texels[0][0] as f32, texels[0][1] as f32, texels[3][0] as f32, texels[3][1] as f32);
    if u1 == u0 || v1 == v0 {
        return;
    }
    let sx = (x1 - x0) / (u1 - u0);
    let sy = (y1 - y0) / (v1 - v0);
    let w = region.wrap;
    let first = (u0.min(u1) / w).floor() as i32;
    let last = (u0.max(u1) / w).ceil() as i32;
    for k in first..last {
        let shift = k as f32 * w;
        let lo = u0.min(u1).max(shift);
        let hi = u0.max(u1).min(shift + w);
        if lo >= hi {
            continue;
        }
        // Sprite space: texel - origin - shift. To the quad's space:
        // (texel - u0) * s + x0.
        let mut t = Transform::IDENTITY;
        t.m[0][0] = sx;
        t.m[1][1] = sy;
        t.m[2][0] = (region.origin[0] + shift - u0) * sx + x0;
        t.m[2][1] = (region.origin[1] - v0) * sy + y0;
        let t = Transform::multiply(transform, &t);
        let clip = [
            lo - shift - region.origin[0],
            v0.min(v1).max(region.rows[0]) - region.origin[1],
            hi - shift - region.origin[0],
            v0.max(v1).min(region.rows[1]) - region.origin[1],
        ];
        cx.draw.sprite_clipped(sprite, &t, clip, Argb(color), Blend::Normal);
    }
}
