//! BackgroundGrid (background.cpp, vtable 0x452878): the scrolling grid
//! behind a game. One tile of 10 x 10 squares, drawn four times around the
//! scroll position so that it wraps. Untextured lines (DrawPoints with
//! D3DPT_LINELIST after ClearTexture): the cooked `shape/grid_tile`.

use super::Cx;
use super::input;
use super::math::sin;
use super::task::Task;
use crate::gfx::shape_table::GRID_HALF_TILE as HALF_TILE;
use crate::gfx::{Argb, Blend, Transform};

pub struct BackgroundGrid {
    pub speed: f32,
    pub color: Argb,
    tiles: [[f32; 2]; 4],
    angle: i16,
    x: f32,
    y: f32,
}

impl BackgroundGrid {
    /// BackgroundGrid (0x415fe0) with the fields fnGame sets.
    pub fn new(color: u32, speed: f32) -> Self {
        BackgroundGrid { speed, color: Argb(color), tiles: [[0.0; 2]; 4], angle: 0, x: 0.0, y: 0.0 }
    }

    /// RotateTransform (0x415f30): turns `t` by `angle` 1024ths of a turn.
    pub fn rotate_by(t: &Transform, angle: i32) -> Transform {
        let mut r = Transform::IDENTITY;
        r.m[0][1] = sin(angle);
        r.m[1][0] = -r.m[0][1];
        r.m[1][1] = sin(angle + 0x100);
        r.m[0][0] = r.m[1][1];
        Transform::multiply(&r, t)
    }
}

impl Task for BackgroundGrid {
    /// Initialize (0x415b90).
    fn initialize(&mut self, cx: &mut Cx) -> bool {
        // The tile's lines are the cooked shape (shape_table::GRID_TILE).
        self.tiles = [[-HALF_TILE, -HALF_TILE], [HALF_TILE, -HALF_TILE], [-HALF_TILE, HALF_TILE], [HALF_TILE, HALF_TILE]];
        self.x = 0.0;
        self.y = 0.0;
        // Takes a number from the C runtime's rand(), as the original.
        self.angle = (cx.rng.rand() & 0x3ff) as i16;
        true
    }

    fn finalize(&mut self, _cx: &mut Cx) {}

    /// IsProtected (0x415e50): turns by the pad every fourth frame (the
    /// live pad, not the replay's keys) and scrolls.
    fn is_protected(&mut self, cx: &mut Cx) -> bool {
        if cx.app.frame_count & 3 == 0 {
            let buttons = cx.input.pad(0).buttons;
            if buttons & input::RIGHT != 0 {
                self.angle = self.angle.wrapping_add(1);
            }
            if buttons & input::LEFT != 0 {
                self.angle = self.angle.wrapping_sub(1);
            }
        }
        let a = self.angle as i32;
        self.x += sin(a) * self.speed;
        self.y -= sin(a + 0x100) * self.speed;
        if self.x < -HALF_TILE {
            self.x += 2.0 * HALF_TILE;
        }
        if self.x > HALF_TILE {
            self.x -= 2.0 * HALF_TILE;
        }
        if self.y < -HALF_TILE {
            self.y += 2.0 * HALF_TILE;
        }
        if self.y > HALF_TILE {
            self.y -= 2.0 * HALF_TILE;
        }
        true
    }

    /// Run (0x416030): the four copies of the tile.
    fn run(&mut self, cx: &mut Cx) {
        for tile in self.tiles {
            let mut t = Transform::IDENTITY;
            t.m[2][0] = -self.x + tile[0];
            t.m[2][1] = -self.y + tile[1];
            let mut t = Self::rotate_by(&t, -(self.angle as i32));
            t.m[2][0] += 320.0;
            t.m[2][1] += 240.0;
            cx.draw.sprite(cx.art.shapes.grid_tile, &t, self.color, Blend::Normal);
        }
    }
}

/// RotateTransform (0x415f30).
pub fn rotate(t: &Transform, angle: i32) -> Transform {
    BackgroundGrid::rotate_by(t, angle)
}
