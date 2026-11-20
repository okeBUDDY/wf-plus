//! BackgroundGrid (background.cpp, vtable 0x452878): the scrolling grid
//! behind a game. One tile of 10 x 10 squares, drawn four times around the
//! scroll position so that it wraps. Untextured lines (DrawPoints with
//! D3DPT_LINELIST after ClearTexture): the tile's twenty lines
//! (`GRID_SQUARES` each way), each drawn as the cooked `shape/grid_line`.

use super::Cx;
use super::input;
use super::math::sin;
use super::task::Task;
use crate::shape_table::{GRID_HALF_TILE as HALF_TILE, GRID_SIZE, GRID_SQUARES};
use crate::gfx::{Argb, Blend, DrawList, Pass, SpriteId, Transform};

/// The tile's four copies, around the scroll position.
const TILES: [[f32; 2]; 4] = [[-HALF_TILE, -HALF_TILE], [HALF_TILE, -HALF_TILE], [-HALF_TILE, HALF_TILE], [HALF_TILE, HALF_TILE]];

/// The interpolation keys of the grid's tiles (above any actor's).
const GRID_KEY: u32 = 0x7fff_0000;

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

    /// A grid already started at `angle` (1024ths of a turn), for screens
    /// outside the task system (the first-run setup screen).
    pub fn started(color: u32, speed: f32, angle: i16) -> Self {
        let mut g = Self::new(color, speed);
        g.tiles = TILES;
        g.angle = angle;
        g
    }

    /// One frame's scroll along the grid's angle, wrapping by a tile.
    pub fn scroll(&mut self) {
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
    }

    /// Run's drawing: the four copies of the tile around (`center_x`, 240).
    pub fn draw(&self, draw: &mut DrawList, grid_line: SpriteId, center_x: f32) {
        // The grid runs on into the margins around the arena; each tile is
        // keyed for display interpolation (a wrap snaps).
        draw.set_pass(Pass::Background);
        for (i, tile) in self.tiles.into_iter().enumerate() {
            draw.set_key(GRID_KEY + i as u32);
            let mut t = Transform::IDENTITY;
            t.m[2][0] = -self.x + tile[0];
            t.m[2][1] = -self.y + tile[1];
            let mut t = Self::rotate_by(&t, -(self.angle as i32));
            t.m[2][0] += center_x;
            t.m[2][1] += 240.0;
            // The rows, then the columns (the line turned a quarter).
            for i in 0..2 * GRID_SQUARES {
                let v = ((i % GRID_SQUARES) * GRID_SIZE) as f32 - HALF_TILE;
                let line = if i < GRID_SQUARES {
                    Transform::translation(0.0, v)
                } else {
                    let mut l = Transform::IDENTITY;
                    l.m[0] = [0.0, 1.0, 0.0];
                    l.m[1] = [-1.0, 0.0, 0.0];
                    l.m[2][0] = v;
                    l
                };
                draw.sprite(grid_line, &Transform::multiply(&t, &line), self.color, Blend::Normal);
            }
        }
        draw.set_key(0);
        draw.set_pass(Pass::Arena);
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
        // The tile: twenty lines, drawn in Run.
        self.tiles = TILES;
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
        self.scroll();
        true
    }

    /// Run (0x416030): the four copies of the tile.
    fn run(&mut self, cx: &mut Cx) {
        self.draw(&mut cx.draw, cx.art.shapes.grid_line, cx.arena.center_x());
    }
}

/// RotateTransform (0x415f30).
pub fn rotate(t: &Transform, angle: i32) -> Transform {
    BackgroundGrid::rotate_by(t, angle)
}
