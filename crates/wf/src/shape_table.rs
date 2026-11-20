// The game's constant untextured shapes: point tables from wf.exe (address
// cited) and the ones its code builds from constants. build.rs includes
// this file and has wf-cook cook each entry of SHAPES into the art pack as
// the sprite `shape/<name>`, drawn like any sprite with its transform on the
// GPU. Plain comments only: the file is `include!`d by the build script.

use wf_art::shape::{Primitive, ShapeDef};

/// g_hexagon (0x45d0a0): the options' and the warning screen chart's hexagon.
pub const HEXAGON: [[f32; 2]; 7] =
    [[0.0, -2.0], [1.732_051, -1.0], [1.732_051, 1.0], [0.0, 2.0], [-1.732_051, 1.0], [-1.732_051, -1.0], [0.0, -2.0]];

/// kMenuCursor (0x45dbe8): the arrows above and below the title menu item.
pub const MENU_CURSOR: [[f32; 2]; 8] =
    [[0.0, -24.0], [-8.0, -16.0], [0.0, -24.0], [8.0, -16.0], [0.0, 24.0], [-8.0, 16.0], [0.0, 24.0], [8.0, 16.0]];

/// kTitleLine (0x45dd88): the line under the boss's title.
pub const TITLE_LINE: [[f32; 2]; 3] = [[-20.0, 20.0], [0.0, 0.0], [360.0, 0.0]];

/// kWeaponBar (0x45de50): a weapon's level bar on the warning screen.
pub const WEAPON_BAR: [[f32; 2]; 4] = [[0.0, 2.0], [64.0, 2.0], [0.0, 12.0], [64.0, 12.0]];

/// kBackdrop (0x45dc8c): the name entry window.
pub const BACKDROP: [[f32; 2]; 4] = [[-320.0, -59.0], [320.0, -59.0], [-320.0, 60.0], [320.0, 60.0]];

/// kNameCursor (0x45dcbc): the underline of the name's current letter.
pub const NAME_CURSOR: [[f32; 2]; 2] = [[0.0, 18.0], [16.0, 18.0]];

/// kCharCursor (0x45dcd4): the box on the chosen character.
pub const CHAR_CURSOR: [[f32; 2]; 4] = [[0.0, 0.0], [8.0, 0.0], [0.0, 8.0], [8.0, 8.0]];

/// g_shockwave_points (0x45d5bc): one triangle of the shockwave ring.
pub const SHOCKWAVE: [[f32; 2]; 3] = [[-18.0, -960.0], [18.0, -960.0], [-2.0, 0.0]];

/// The background grid's tile (BackgroundGrid::Initialize, 0x415b90): 10 x
/// 10 squares of 96, centred: ten rows and ten columns of lines.
pub const GRID_SQUARES: i32 = 10;
pub const GRID_SIZE: i32 = 96;
pub const GRID_HALF_TILE: f32 = (GRID_SQUARES * GRID_SIZE / 2) as f32;
/// One line of the grid tile, across it (drawn ten times each way per
/// tile): one sprite per line keeps each pixel's rays to the few curves of
/// its own line, where the whole tile's bands held every crossing line.
pub const GRID_LINE: [[f32; 2]; 2] = [[-GRID_HALF_TILE, 0.0], [GRID_HALF_TILE, 0.0]];

/// The screen fade's rectangle (Sprite, sprite.cpp): the whole 640x480 screen.
pub const SCREEN: [[f32; 2]; 4] = [[0.0, 0.0], [640.0, 0.0], [0.0, 480.0], [640.0, 480.0]];

const fn def(name: &'static str, primitive: Primitive, points: &'static [[f32; 2]], min_scale: f32) -> ShapeDef {
    ShapeDef { name, primitive, points, min_scale }
}

/// Every constant shape. `min_scale` is the smallest scale a line shape is
/// drawn at (the hexagons grow from nothing; below 4 px per unit they are a
/// few pixels wide anyway).
pub const SHAPES: &[ShapeDef] = &[
    def("hexagon", Primitive::LineStrip, &HEXAGON, 4.0),
    def("menu_cursor", Primitive::LineList, &MENU_CURSOR, 1.0),
    def("title_line", Primitive::LineStrip, &TITLE_LINE, 1.0),
    def("weapon_bar", Primitive::TriangleStrip, &WEAPON_BAR, 1.0),
    def("backdrop", Primitive::TriangleStrip, &BACKDROP, 1.0),
    def("name_cursor", Primitive::LineList, &NAME_CURSOR, 1.0),
    def("char_cursor", Primitive::TriangleStrip, &CHAR_CURSOR, 1.0),
    def("shockwave", Primitive::TriangleStrip, &SHOCKWAVE, 1.0),
    def("grid_line", Primitive::LineList, &GRID_LINE, 1.0),
    def("screen", Primitive::TriangleStrip, &SCREEN, 1.0),
];
