//! GameHud (game_hud*.cpp, vtable 0x453494): the in-game display, an
//! overlay task of fnGame. Besides the status texts it runs one screen at
//! a time: the game over screen, the name entry and the boss result.

use super::Cx;
use super::game_state::FLAG_ENGLISH;
use super::input;
use super::math::sin;
use super::score_table::RANK_NAMES;
use super::sounds;
use super::task::Task;
use super::texels;
use crate::shape_table::HEXAGON;
use crate::gfx::{Argb, Blend, Primitive, Transform};

pub const MODE_NONE: i32 = 0;
pub const MODE_GAME_OVER: i32 = 1;
pub const MODE_NAME_ENTRY: i32 = 2;
pub const MODE_RESULT: i32 = 3;

const FLAG_NAME_CURSOR: u32 = 0x1;
const FLAG_SKIP: u32 = 0x100;
const FLAG_SCORE: u32 = 0x10000;
const FLAG_TIMER: u32 = 0x20000;
const FLAG_LIVES: u32 = 0x40000;
const FLAG_REPLAY: u32 = 0x100_0000;

/// The colour of the status texts.
const TEXT_COLOR: u32 = 0xc8ff_c800;

/// The requests fnGame makes of the HUD (RequestMode and busy()).
#[derive(Default, Clone, Copy, Debug)]
pub struct HudState {
    pub mode: i32,
    pub next_mode: i32,
}

impl HudState {
    pub fn request_mode(&mut self, mode: i32) {
        self.next_mode = mode;
    }
    pub fn busy(&self) -> bool {
        self.next_mode | self.mode != 0
    }
}

// The game over screen's tables.
const BAND: [[f32; 2]; 4] = [[-256.0, -24.0], [768.0, -24.0], [-256.0, 24.0], [768.0, 24.0]]; // 0x45dd48
const BAND_TEXELS: [[i16; 2]; 4] = [[0, 208], [1024, 208], [0, 256], [1024, 256]]; // 0x45dd78
const WEAPON_ICON: [[f32; 2]; 4] = [[-16.0, 0.0], [64.0, 0.0], [-16.0, 24.0], [64.0, 24.0]]; // 0x45ddac
const WEAPON_TEXELS: [[[i16; 2]; 4]; 7] = [
    [[96, 128], [176, 128], [96, 152], [176, 152]],
    [[96, 152], [176, 152], [96, 176], [176, 176]],
    [[96, 176], [176, 176], [96, 200], [176, 200]],
    [[96, 200], [176, 200], [96, 224], [176, 224]],
    [[96, 224], [176, 224], [96, 248], [176, 248]],
    [[176, 128], [256, 128], [176, 152], [256, 152]],
    [[176, 152], [256, 152], [176, 176], [256, 176]],
]; // 0x45dde0

// The name entry's tables.
const NAME_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789 %&+-/!?.,_ <$"; // 0x45dc48
const BAR_POINTS: [[f32; 2]; 4] = [[-320.0, -8.0], [448.0, -8.0], [-320.0, 8.0], [448.0, 8.0]]; // 0x45dc4c
const BAR_TEXELS: [[i16; 2]; 4] = [[0, 32], [768, 32], [0, 48], [768, 48]]; // 0x45dc7c
const CHAR_LAST_SIGN: i32 = 47;
const CHAR_DELETE: i32 = 49;
const CHAR_END: i32 = 50;
const CHARS: i32 = 51;
const NAME_LENGTH: i32 = 3;

// The result's tables.
const RESULT_BAR: [[f32; 2]; 4] = [[0.0, -8.0], [768.0, -8.0], [0.0, 8.0], [768.0, 8.0]]; // 0x45dd04
const RESULT_BAR_TEXELS: [[i16; 2]; 4] = [[0, 0], [768, 0], [0, 16], [768, 16]]; // 0x45dd34

/// The skip buttons (fire, slow, weapon).
const SKIP_KEYS: u16 = input::SHOT | input::SLOW | input::WEAPON_SELECT;

/// Lerp (0x41f560).
fn lerp(out: &mut f32, from: f32, to: f32, frames: i32, time: i32) -> bool {
    if time >= frames {
        *out = to;
        return true;
    }
    *out = (to - from) * time as f32 / frames as f32 + from;
    false
}

/// Lerp (0x420250).
fn lerp_int(out: &mut i32, from: i32, to: i32, frames: i32, time: i32) -> bool {
    if time >= frames {
        *out = to;
        return true;
    }
    *out = (to - from) * time / frames + from;
    false
}

fn alpha(a: f32, k: f32) -> u32 {
    ((super::math::ftol(a * k)) as u32) << 24
}

#[derive(Default)]
pub struct GameHud {
    flags: u32,
    step: i32,
    mode_time: i32,
    replay_name_x: i32,
    replay_name: Vec<u8>,
    replay_date: Vec<u8>,
    texts: [Vec<u8>; 4],
    // The name entry.
    cursor: i32,
    char_index: i32,
    back_alpha: f32,
    text_alpha: f32,
    open: f32,
    name: [u8; 4],
    last_dirs: u16,
    dirs_time: i16,
    // The result.
    result_bar: f32,
    result_alpha: f32,
    result_height: i32,
    result_shown: i16,
    result_length: i16,
    // The game over screen.
    title_pos: (i32, i32),
    title_slide: f32,
    frame_open: f32,
    band_height: f32,
    chart_alpha: f32,
    info_alpha: f32,
    param_count: usize,
    params: [i16; 16],
    shown_params: [i16; 16],
    /// chart_center_ and chart_[7], drawn as one fan.
    chart: [[f32; 2]; 8],
}

impl GameHud {
    pub fn new() -> Self {
        Self::default()
    }

    fn mode_flags_clear(&mut self) {
        self.flags &= !0xff;
    }

    /// SetMode (0x41eef0).
    fn set_mode(&mut self, cx: &mut Cx, mode: i32) {
        cx.hud.mode = mode;
        self.step = 0;
        self.mode_time = 0;
        self.mode_flags_clear();
        self.texts = Default::default();
    }

    /// UpdateChart (0x420330).
    fn update_chart(&mut self) {
        for i in 0..6 {
            let scale = self.shown_params[i] as f32 * 0.0625;
            self.chart[1 + i] = [HEXAGON[i][0] * scale, HEXAGON[i][1] * scale];
        }
        self.chart[7] = self.chart[1];
    }

    /// LoadParams (0x4204b0).
    fn load_params(&mut self, cx: &Cx) {
        self.param_count = cx.enm.get_params(&mut self.params);
        for i in 0..self.param_count {
            if self.params[i] < 0x40 {
                self.params[i] = 0x40;
            }
            self.shown_params[i] = 0;
        }
        self.shown_params[0] = self.params[0];
        self.update_chart();
    }

    fn end_step(&mut self, step: i32) {
        if step != self.step {
            self.mode_time = 0;
        } else {
            self.mode_time += 1;
        }
    }

    /// MoveGameOver (0x420bd0).
    fn move_game_over(&mut self, cx: &mut Cx) -> bool {
        let step = self.step;
        match step {
            0 => {
                self.load_params(cx);
                self.frame_open = 0.0;
                self.band_height = 0.0;
                self.info_alpha = 1.0;
                self.chart_alpha = 0.0;
                self.title_slide = 400.0;
                self.texts[0] = b"NAME:".to_vec();
                self.texts[1] = format!("STAGE:{:02}", cx.global.stage).into_bytes();
                let english = cx.global.flags & FLAG_ENGLISH != 0;
                self.texts[2] = if english { cx.enm.format_title_en(&cx.fonts.title) } else { cx.enm.format_title_ja() };
                let font = if english { &cx.fonts.title } else { &cx.fonts.kana };
                let (_, h) = font.measure(&self.texts[2]);
                self.title_pos = (self.title_slide as i32 + 360, 400 - h as i32);
                cx.play_sound(sounds::WARNING);
                self.step += 1;
            }
            1 => {
                let mut angle = 0;
                if lerp_int(&mut angle, 0, 0x100, 20, self.mode_time) {
                    self.step = 2;
                }
                self.frame_open = sin(angle);
            }
            2 => {
                if lerp(&mut self.band_height, 0.0, 78.0, 20, self.mode_time) {
                    self.step += 1;
                }
            }
            3 => {
                if lerp(&mut self.chart_alpha, 0.0, 1.0, 10, self.mode_time) {
                    self.flags &= !FLAG_SKIP;
                    self.step += 1;
                }
            }
            4 => {
                // Grow the chart one corner at a time.
                let mut done = 0;
                for i in 0..self.param_count {
                    if self.shown_params[i] < self.params[i] {
                        self.shown_params[i] += 0x40;
                        if self.shown_params[i] > self.params[i] {
                            self.shown_params[i] = self.params[i];
                        }
                        self.update_chart();
                        if i < 6 {
                            break;
                        }
                    } else {
                        done += 1;
                    }
                }
                let mut angle = 0;
                if lerp_int(&mut angle, 0, 0x100, 20, self.mode_time) && done >= self.param_count {
                    self.step = 10;
                }
                self.title_slide = (1.0 - sin(angle)) * 400.0;
            }
            10 => {
                if self.mode_time >= 180 || self.flags & FLAG_SKIP != 0 {
                    self.step = 20;
                }
            }
            20 => {
                lerp(&mut self.chart_alpha, 1.0, 0.0, 20, self.mode_time);
                if lerp(&mut self.info_alpha, 1.0, 0.0, 20, self.mode_time) {
                    self.step += 1;
                }
            }
            21 => {
                lerp(&mut self.title_slide, 0.0, 400.0, 20, self.mode_time);
                lerp(&mut self.frame_open, 1.0, 0.0, 20, self.mode_time);
                if lerp(&mut self.band_height, 78.0, 0.0, 20, self.mode_time) {
                    return true;
                }
            }
            _ => {}
        }
        self.end_step(step);
        false
    }

    /// DrawGameOver (0x420500).
    fn draw_game_over(&mut self, cx: &mut Cx) {
        let mut t = Transform::IDENTITY;
        t.scale(1.0, self.frame_open);
        t.translate(-(((cx.app.frame_count & 0x3f) * 4) as f32), -200.0);
        // The original's literal (pi / 6 rounded to f32).
        #[allow(clippy::approx_constant)]
        t.rotate_z(0.523_598_8);
        let mut band = t;
        band.m[2][0] += 160.0;
        band.m[2][1] += 200.0;
        for k in band_copies(cx) {
            texels::draw(cx, &texels::ETC0_BAND, &along(&band, k), &BAND, &BAND_TEXELS, 0xc8ff_4000);
        }
        let mut t = super::background::rotate(&t, 0x200);
        t.translate(160.0, 200.0);
        for k in band_copies(cx) {
            texels::draw(cx, &texels::ETC0_BAND, &along(&t, k), &BAND, &BAND_TEXELS, 0xc8ff_4000);
        }

        let chart_color = alpha(self.chart_alpha, 200.0) | 0xff80;
        let info_color = alpha(self.info_alpha, 128.0) | 0xff_8000;
        let t = Transform::translation(160.0, 200.0);
        let chart = cx.pats.get(super::actor::sheet::ETC0 + 1, 1);
        super::actor::draw_obj(cx, chart, &t, chart_color, Blend::Normal);
        cx.draw.shape(Primitive::TriangleFan, &t, &self.chart, Argb(info_color), Blend::Normal);
        if self.band_height > 0.0 {
            let mut t = Transform::IDENTITY;
            t.scale(self.band_height, self.band_height);
            let mut t = super::background::rotate(&t, cx.app.frame_count.wrapping_mul(3).wrapping_neg() as i32);
            t.translate(160.0, 200.0);
            cx.draw.sprite(cx.art.shapes.hexagon, &t, Argb(0xc800_ff80), Blend::Normal);
        }

        let count = cx.enm.state.weapon_count.max(0) as usize;
        let (mut x, mut y) = (360.0f32, 172.0f32);
        for i in 0..count.min(8) {
            let w = (cx.enm.state.weapons[i] as usize).min(6);
            let t = Transform::translation(x, y);
            texels::draw(cx, &texels::ETC2_WEAPONS, &t, &WEAPON_ICON, &WEAPON_TEXELS[w], chart_color);
            x -= 21.0;
            y += 36.0;
        }
        let (mut x, mut y) = (360.0f32, 172.0f32);
        for i in 0..count.min(8) {
            let mut t = Transform::IDENTITY;
            t.scale(self.shown_params[6 + i] as f32 / 1024.0, 1.0);
            t.translate(x, y);
            cx.draw.sprite(cx.art.shapes.weapon_bar, &t, Argb(info_color), Blend::Normal);
            x -= 21.0;
            y += 36.0;
        }

        let t = Transform::translation(self.title_slide + 360.0, 404.0);
        self.title_pos.0 = self.title_slide as i32 + 360;
        cx.draw.sprite(cx.art.shapes.title_line, &t, Argb(0xc8ff_c800), Blend::Normal);
        let (px, py) = (self.title_pos.0 as f32, self.title_pos.1 as f32);
        let c = Argb(0xc8ff_c800);
        cx.fonts.text.print(&mut cx.draw, px, py - 20.0, &self.texts[0], None, c, -1, Blend::Normal);
        let english = cx.global.flags & FLAG_ENGLISH != 0;
        let font = if english { &cx.fonts.title } else { &cx.fonts.kana };
        font.print(&mut cx.draw, px + 20.0, py, &self.texts[2], None, c, -1, Blend::Normal);
        cx.fonts.text.print(&mut cx.draw, px + 144.0, 408.0, &self.texts[1], None, c, -1, Blend::Normal);
    }

    /// EnterName (0x41f600): true once the name is ended.
    fn enter_name(&mut self, cx: &mut Cx) -> bool {
        let pad = cx.input.pad(0);
        let mut pressed = pad.pressed;
        let dirs = pad.buttons & (input::UP | input::DOWN | input::LEFT | input::RIGHT);
        if self.last_dirs == dirs {
            self.dirs_time += 1;
            if self.dirs_time > 30 && self.dirs_time & 3 == 0 {
                pressed |= dirs;
            }
        } else {
            self.dirs_time = 0;
            self.last_dirs = dirs;
        }
        let text = &mut self.texts[2];
        if pressed & (input::UP | input::DOWN | input::LEFT | input::RIGHT) != 0 {
            // No delete or end at the first letter, only those after the last.
            let (first, last) = match self.cursor {
                0 => (0, CHAR_LAST_SIGN),
                NAME_LENGTH => (CHAR_DELETE, CHAR_END),
                _ => (0, CHAR_END),
            };
            if pressed & (input::DOWN | input::RIGHT) != 0 {
                self.char_index += 1;
                if self.char_index > last {
                    self.char_index = first;
                }
                while NAME_CHARS[self.char_index as usize] == b' ' {
                    self.char_index += 1;
                }
            } else {
                self.char_index -= 1;
                if self.char_index < first {
                    self.char_index = last;
                }
                while NAME_CHARS[self.char_index as usize] == b' ' {
                    self.char_index -= 1;
                }
            }
            cx.play_sound(sounds::FIRE_ME);
        } else {
            let mut index = self.char_index;
            if pressed & (input::SHOT | input::SLOW) == input::SLOW {
                index = CHAR_DELETE;
                pressed |= input::SHOT;
            }
            if pressed & input::SHOT != 0 {
                cx.play_sound(sounds::SWITCH);
                match index {
                    CHAR_DELETE => {
                        if self.cursor > 0 {
                            text[self.cursor as usize] = if self.cursor >= NAME_LENGTH { b' ' } else { b'-' };
                            self.cursor -= 1;
                            for i in 0..CHARS {
                                if self.name[self.cursor as usize] == NAME_CHARS[i as usize] {
                                    self.char_index = i;
                                }
                            }
                        }
                    }
                    CHAR_END => {
                        self.name[self.cursor as usize] = 0;
                        text.truncate(self.cursor as usize);
                        return true;
                    }
                    _ => {
                        self.name[self.cursor as usize] = NAME_CHARS[self.char_index as usize];
                        self.cursor += 1;
                        if self.cursor >= NAME_LENGTH {
                            self.char_index = CHAR_END;
                        }
                    }
                }
            }
        }
        let text = &mut self.texts[2];
        text[self.cursor as usize] = NAME_CHARS[self.char_index as usize];
        false
    }

    /// MoveNameEntry (0x41fc90).
    fn move_name_entry(&mut self, cx: &mut Cx) -> bool {
        let step = self.step;
        match step {
            0 => {
                let rank = cx.scores.find_rank(&cx.global.entry);
                if rank < 0 {
                    return true;
                }
                self.texts[0] = format!("{}: {:012}", RANK_NAMES[rank as usize], cx.global.entry.score as i32).into_bytes();
                self.texts[1] = b"PLEASE ENTER YOUR NAME!".to_vec();
                self.texts[2] = b"A-- ".to_vec();
                self.texts[3] = NAME_CHARS.to_vec();
                self.cursor = 0;
                self.char_index = 0;
                self.back_alpha = 0.0;
                self.text_alpha = 0.0;
                self.open = 0.0;
                self.name = [0; 4];
                self.step += 1;
            }
            1 => {
                if lerp(&mut self.open, 0.0, 1.0, 10, self.mode_time) {
                    self.step += 1;
                }
            }
            2 => {
                if lerp(&mut self.back_alpha, 0.0, 1.0, 10, self.mode_time) {
                    self.step += 1;
                }
            }
            3 => {
                if lerp(&mut self.text_alpha, 0.0, 1.0, 10, self.mode_time) {
                    self.step = 10;
                }
            }
            10 | 11 => {
                if step == 10 {
                    self.last_dirs = 0;
                    self.dirs_time = 0;
                    self.flags |= FLAG_NAME_CURSOR;
                    self.step = 11;
                }
                if self.enter_name(cx) {
                    self.flags &= !FLAG_NAME_CURSOR;
                    cx.global.entry.name = self.name;
                    let entry = cx.global.entry;
                    cx.scores.arena = cx.arena.size_u16();
                    cx.scores.insert(&entry, -1);
                    cx.scores.store();
                    self.step = 20;
                }
            }
            20 => {
                lerp(&mut self.text_alpha, 1.0, 0.0, 10, self.mode_time);
                if lerp(&mut self.back_alpha, 1.0, 0.0, 10, self.mode_time) {
                    self.step += 1;
                }
            }
            21
                if lerp(&mut self.open, 1.0, 0.0, 10, self.mode_time) => {
                    return true;
                }
            _ => {}
        }
        self.end_step(step);
        false
    }

    /// DrawNameEntry (0x41f830).
    fn draw_name_entry(&mut self, cx: &mut Cx) {
        // Centred in a wider arena.
        cx.draw.set_origin([cx.arena.center_offset_x(320.0) - 320.0, 0.0]);
        let t = Transform::translation(320.0, 240.0);
        cx.draw.sprite(cx.art.shapes.backdrop, &t, Argb(alpha(self.back_alpha, 160.0)), Blend::Normal);
        // The bars, scrolling apart as the window opens.
        let mut t = Transform::IDENTITY;
        t.scale(1.0, self.open);
        t.m[2][0] = -(((cx.app.frame_count & 0x3f) * 2) as f32);
        t.m[2][1] -= 52.0;
        let mut turned = super::background::rotate(&t, 0x200);
        t.m[2][0] += 320.0;
        t.m[2][1] += 240.0;
        let strip = texels::font2_strip(2);
        for k in band_copies(cx) {
            texels::draw(cx, &strip, &along(&t, k), &BAR_POINTS, &BAR_TEXELS, 0xc800_ff80);
        }
        turned.m[2][0] += 320.0;
        turned.m[2][1] += 240.0;
        for k in band_copies(cx) {
            texels::draw(cx, &strip, &along(&turned, k), &BAR_POINTS, &BAR_TEXELS, 0xc800_ff80);
        }

        if self.flags & FLAG_NAME_CURSOR != 0 {
            let t = Transform::translation(self.cursor as f32 * 16.0 + 296.0, 244.0);
            let a = super::math::ftol((1.0 - (self.mode_time & 0x3f) as f32 * (1.0 / 63.0)) * 200.0) as u32;
            cx.draw.sprite(cx.art.shapes.name_cursor, &t, Argb((a << 24) | 0xffc800), Blend::Normal);
            let t = Transform::translation(self.char_index as f32 * 8.0 + 116.0, 268.0);
            cx.draw.sprite(cx.art.shapes.char_cursor, &t, Argb(0x6400_ff80), Blend::Normal);
        }
        let color = Argb(alpha(self.text_alpha, 200.0) | 0xffc800);
        let white = Argb(alpha(self.text_alpha, 128.0) | 0xff_ffff);
        cx.fonts.text.print(&mut cx.draw, 184.0, 204.0, &self.texts[0], None, color, -1, Blend::Normal);
        cx.fonts.small_text.print(&mut cx.draw, 228.0, 228.0, &self.texts[1], None, white, -1, Blend::Normal);
        cx.fonts.text.print(&mut cx.draw, 296.0, 244.0, &self.texts[2], None, color, -1, Blend::Normal);
        cx.fonts.small_text.print(&mut cx.draw, 116.0, 268.0, &self.texts[3], None, white, -1, Blend::Normal);
        // The table the name goes into (display only).
        let mode = mode_label(cx);
        cx.fonts.small_text.print(&mut cx.draw, 320.0 - 4.0 * mode.len() as f32, 288.0, mode.as_bytes(), None, white, -1, Blend::Normal);
        cx.draw.set_origin([0.0; 2]);
    }

    /// MoveResult (0x41ff20).
    fn move_result(&mut self, cx: &mut Cx) -> bool {
        let step = self.step;
        match step {
            0 => {
                let rules = cx.game.rules;
                let (text, columns, lines) = cx.enm.format_result(cx.global.stage, &rules);
                self.texts[0] = text.into_bytes();
                self.result_bar = 0.0;
                self.result_alpha = 1.0;
                self.result_shown = 0;
                self.result_length = (lines * columns) as i16;
                self.result_height = lines * 16;
                self.step += 1;
            }
            1 => {
                if self.mode_time > 30 {
                    self.step = 2;
                }
            }
            2 => {
                self.result_shown += 1;
                if self.result_shown > self.result_length {
                    self.flags &= !FLAG_SKIP;
                    self.step = 3;
                }
            }
            3 => {
                if lerp(&mut self.result_bar, 0.0, 1.0, 10, self.mode_time) {
                    self.step = 10;
                }
            }
            10 => {
                if self.flags & FLAG_SKIP != 0 {
                    cx.play_sound(sounds::SWITCH);
                    self.step = 20;
                }
            }
            20 => {
                lerp(&mut self.result_alpha, 1.0, 0.0, 10, self.mode_time);
                if lerp(&mut self.result_bar, 1.0, 0.0, 10, self.mode_time) {
                    self.step += 1;
                }
            }
            21
                if self.mode_time > 30 => {
                    return true;
                }
            _ => {}
        }
        self.end_step(step);
        false
    }

    /// DrawResult (0x4200e0).
    fn draw_result(&mut self, cx: &mut Cx) {
        let mut t = Transform::IDENTITY;
        t.scale(1.0, self.result_bar);
        t.translate(-(((cx.app.frame_count & 0x3f) * 2) as f32), 460.0);
        for k in band_copies(cx) {
            texels::draw(cx, &texels::font2_strip(0), &along(&t, k), &RESULT_BAR, &RESULT_BAR_TEXELS, 0xc800_ff80);
        }
        let mut palette = [Argb(0); 10];
        palette[0] = Argb(alpha(self.result_alpha, 128.0) | 0xff_ffff);
        palette[1] = Argb(alpha(self.result_alpha, 200.0) | 0xffc800);
        let len = (self.result_length as i32).max(1);
        let y = 448 - self.result_height * self.result_shown as i32 / len;
        cx.fonts.small_text.print(&mut cx.draw, 16.0, y as f32, &self.texts[0], Some(&palette), palette[0], self.result_shown as i32, Blend::Normal);
    }
}

impl Task for GameHud {
    /// Initialize (0x41f060).
    fn initialize(&mut self, cx: &mut Cx) -> bool {
        self.flags = 0xff_0000;
        cx.hud = HudState::default();
        self.chart = [[0.0; 2]; 8];
        if cx.replay.mode == super::replay::Mode::Play
            && let Ok(meta) = std::fs::metadata(cx.replay.path()) {
                let mut name: Vec<u8> = cx.replay.path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().into_bytes();
                self.replay_name_x = (80 - name.len() as i32) * 8;
                for c in &mut name {
                    *c = c.to_ascii_uppercase();
                    if *c < b' ' || *c > b'_' {
                        *c = b'?';
                    }
                }
                self.replay_name = name;
                self.replay_date = meta.modified().ok().map(super::time::format_local).unwrap_or_default().into_bytes();
                self.flags |= FLAG_REPLAY;
            }
        true
    }

    /// Finalize (0x41f280).
    fn finalize(&mut self, cx: &mut Cx) {
        cx.hud = HudState::default();
        self.flags = 0;
    }

    /// IsProtected (0x41ef80).
    fn is_protected(&mut self, cx: &mut Cx) -> bool {
        if cx.hud.next_mode != MODE_NONE {
            let m = cx.hud.next_mode;
            self.set_mode(cx, m);
            cx.hud.next_mode = MODE_NONE;
        }
        if cx.fn_game.pressed & SKIP_KEYS != 0 {
            self.flags |= FLAG_SKIP;
        }
        let done = match cx.hud.mode {
            MODE_GAME_OVER => self.move_game_over(cx),
            MODE_NAME_ENTRY => self.move_name_entry(cx),
            MODE_RESULT => self.move_result(cx),
            _ => false,
        };
        if done {
            cx.hud.mode = MODE_NONE;
        }
        true
    }

    /// Run (0x41f340).
    fn run(&mut self, cx: &mut Cx) {
        match cx.hud.mode {
            MODE_GAME_OVER => self.draw_game_over(cx),
            MODE_NAME_ENTRY => self.draw_name_entry(cx),
            MODE_RESULT => self.draw_result(cx),
            _ => {}
        }
        let c = Argb(TEXT_COLOR);
        if self.flags & FLAG_SCORE != 0 {
            let s = format!("1P:{:012}", cx.global.entry.score as i32);
            cx.fonts.text.print(&mut cx.draw, 0.0, 0.0, s.as_bytes(), None, c, -1, Blend::Normal);
        }
        if self.flags & FLAG_LIVES != 0 {
            let (lives, extend) = (cx.game.lives, cx.game.extend_count);
            if lives >= 0 {
                let s = format!("LEFT:{lives}");
                cx.fonts.text.print(&mut cx.draw, 0.0, 20.0, s.as_bytes(), None, c, -1, Blend::Normal);
                if extend >= 0 {
                    let s = format!(".{extend:02}");
                    cx.fonts.small_text.print(&mut cx.draw, 96.0, 28.0, s.as_bytes(), None, c, -1, Blend::Normal);
                }
            }
        }
        // The timer and the replay's name keep to the right edge.
        cx.draw.set_origin([cx.arena.right_x(640.0) - 640.0, 0.0]);
        if self.flags & FLAG_TIMER != 0 {
            let seconds = cx.game.timer / 60;
            if seconds >= 100 {
                let s = format!("{seconds:03}");
                cx.fonts.large_digit.print(&mut cx.draw, 448.0, 0.0, s.as_bytes(), None, c, -1, Blend::Normal);
            } else {
                let s = format!("{seconds:02}");
                cx.fonts.large_digit.print(&mut cx.draw, 496.0, 0.0, s.as_bytes(), None, c, -1, Blend::Normal);
            }
            let s = format!("{:02}", cx.game.timer % 60);
            cx.fonts.digit.print(&mut cx.draw, 592.0, 1.0, s.as_bytes(), None, c, -1, Blend::Normal);
        }
        if self.flags & FLAG_REPLAY == 0 {
            cx.draw.set_origin([0.0; 2]);
        }
        if self.flags & FLAG_REPLAY != 0 {
            cx.fonts.small_text.print(&mut cx.draw, self.replay_name_x as f32, 456.0, &self.replay_name, None, c, -1, Blend::Normal);
            cx.fonts.small_text.print(&mut cx.draw, 512.0, 468.0, &self.replay_date, None, c, -1, Blend::Normal);
            cx.draw.set_origin([0.0; 2]);
            if cx.fn_game.mode == 1 {
                let help = b"SHOT BUTTON -> NEXT REPLAY\nSLOW BUTTON -> GO TO TITLE";
                cx.fonts.small_text.print(&mut cx.draw, 0.0, 460.0, help, None, c, -1, Blend::Normal);
            }
        }
    }
}

/// The copies of a scrolling band (768 units of texels long) that cover
/// the arena: one in Classic; in a wider HD arena, more on each side
/// along the band.
fn band_copies(cx: &Cx) -> std::ops::RangeInclusive<i32> {
    let extra = (cx.arena.width - 640.0).max(0.0);
    let k = (extra / 600.0).ceil() as i32;
    -k..=k
}

/// `t` moved `k` band lengths along its own x.
fn along(t: &Transform, k: i32) -> Transform {
    let mut t = *t;
    if k != 0 {
        let d = 768.0 * k as f32;
        t.m[2][0] += t.m[0][0] * d;
        t.m[2][1] += t.m[0][1] * d;
    }
    t
}

/// The arena mode of the scores, as the score screens show it.
pub fn mode_label(cx: &super::Cx) -> String {
    match cx.arena.mode {
        super::arena::Mode::Classic => "CLASSIC SCORES".to_string(),
        super::arena::Mode::Hd => format!("HD SCORES ({}X{})", cx.arena.width, cx.arena.height),
    }
}
