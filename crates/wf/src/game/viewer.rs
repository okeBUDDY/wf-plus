//! The art and sound viewer: a stand-in first stage while the game's own
//! stages are being ported. It pages through every cooked sprite, shows
//! the fonts, and plays the sound effects.
//!
//! Left/Right: page, Up/Down: select, shot (Z / pad A): play the next sound,
//! slow (X / pad B): toggle additive blending, Esc: quit.

use super::Cx;
use super::app::Stage;
use super::assets::{CHANNEL_BGM, SOUNDS};
use super::fade::ScreenFade;
use super::input::{DOWN, LEFT, RIGHT, SHOT, SLOW, UP};
use super::text::{self, Fonts};
use crate::gfx::{Argb, Blend, SpriteId, Transform};

const COLUMNS: usize = 6;
const ROWS: usize = 5;
const CELL: f32 = 64.0;
const PER_PAGE: usize = COLUMNS * ROWS;

pub struct Viewer {
    sprites: Vec<SpriteId>,
    page: usize,
    selected: usize,
    additive: bool,
    sound: usize,
    last_sound: String,
}

impl Viewer {
    pub fn new() -> Self {
        Viewer { sprites: Vec::new(), page: 0, selected: 0, additive: false, sound: 0, last_sound: String::new() }
    }

    fn pages(&self) -> usize {
        self.sprites.len().div_ceil(PER_PAGE) + 1
    }

    fn fonts_page(&self) -> bool {
        self.page + 1 == self.pages()
    }
}

impl Default for Viewer {
    fn default() -> Self {
        Self::new()
    }
}

/// Fits `bounds` into a `size` box centred on (`cx`, `cy`).
fn fit(bounds: [f32; 4], size: f32, cx: f32, cy: f32, rotate: f32) -> Transform {
    let w = (bounds[2] - bounds[0]).max(1.0);
    let h = (bounds[3] - bounds[1]).max(1.0);
    let s = (size / w.max(h)).min(4.0);
    let mut t = Transform::translation(-(bounds[0] + bounds[2]) * 0.5, -(bounds[1] + bounds[3]) * 0.5);
    t.scale(s, s);
    if rotate != 0.0 {
        t.rotate_z(rotate);
    }
    t.translate(cx, cy);
    t
}

impl Stage for Viewer {
    fn initialize(&mut self, cx: &mut Cx) -> bool {
        self.sprites = (0..cx.art.count() as u32)
            .map(SpriteId)
            .filter(|&id| !cx.art.name(id).starts_with("font/"))
            .collect();
        if let Some(fade) = cx.screen_fade.and_then(|id| cx.overlays.get_mut::<ScreenFade>(id)) {
            fade.fade_in(30);
        }
        if let Some(bgm) = &cx.assets.bgm {
            cx.sound.play(CHANNEL_BGM, bgm, true, 1.0);
        }
        true
    }

    fn update(&mut self, cx: &mut Cx) {
        let pad = cx.input.pad(0);
        let (pressed, pages) = (pad.pressed, self.pages());
        if pressed & RIGHT != 0 {
            self.page = (self.page + 1) % pages;
            self.selected = 0;
        }
        if pressed & LEFT != 0 {
            self.page = (self.page + pages - 1) % pages;
            self.selected = 0;
        }
        let on_page = self.sprites.len().saturating_sub(self.page * PER_PAGE).min(PER_PAGE).max(1);
        if pressed & DOWN != 0 {
            self.selected = (self.selected + 1) % on_page;
        }
        if pressed & UP != 0 {
            self.selected = (self.selected + on_page - 1) % on_page;
        }
        if pressed & SLOW != 0 {
            self.additive = !self.additive;
        }
        if pressed & SHOT != 0 {
            let (channel, name) = SOUNDS[self.sound % SOUNDS.len()];
            if let Some(Some(sound)) = cx.assets.sounds.get(channel) {
                cx.sound.play(channel, sound, false, 1.0);
            }
            self.last_sound = name.to_string();
            self.sound += 1;
        }
    }

    fn draw(&mut self, cx: &mut Cx) {
        let white = Argb::WHITE;
        let blend = if self.additive { Blend::Additive } else { Blend::Normal };
        let frame = cx.app.frame_count as f32;
        if self.fonts_page() {
            let f = &cx.fonts;
            let d = &mut cx.draw;
            let pr = |font: &super::text::Font, d: &mut crate::gfx::DrawList, x, y, s: &str, c| {
                font.print(d, x, y, &text::sjis(s), None, c, -1, Blend::Normal)
            };
            pr(&f.title, d, 24.0, 24.0, "WARNING FOREVER", white);
            pr(&f.text, d, 24.0, 72.0, "ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789", white);
            pr(&f.text, d, 24.0, 92.0, "@@4GREEN @@2RED @@5CYAN @@9GREY@@ WHITE", white);
            pr(&f.small_text, d, 24.0, 120.0, "SMALL TEXT: THE QUICK BROWN FOX 0123456789 !\"#$%&'()*+,-./:;<=>?", white);
            pr(&f.kana, d, 24.0, 140.0, "ワーニング　フォーエバー★☆", white);
            pr(&f.large_digit, d, 24.0, 190.0, "0123456789", white);
            pr(&f.digit, d, 24.0, 290.0, "0123456789", Argb(0xffffc000));
            let mut t = Transform::IDENTITY;
            t.scale(3.0, 3.0);
            t.translate(24.0, 350.0);
            // Scaled glyphs stay sharp: they are Slug outlines.
            if let Some(g) = cx.art.find("font/text/033") {
                for i in 0..6 {
                    let mut ti = t;
                    ti.translate(i as f32 * 50.0, 0.0);
                    if let Some(gi) = cx.art.find(&format!("font/text/{:03}", 33 + i)) {
                        cx.draw.sprite(gi, &ti, white, Blend::Normal);
                    }
                }
                let _ = g;
            }
        } else {
            let start = self.page * PER_PAGE;
            let end = (start + PER_PAGE).min(self.sprites.len());
            for (k, &id) in self.sprites[start..end].iter().enumerate() {
                let (col, row) = (k % COLUMNS, k / COLUMNS);
                let (x, y) = (16.0 + CELL * col as f32 + CELL * 0.5, 40.0 + CELL * row as f32 + CELL * 0.5);
                let bounds = cx.art.pack.sprites[id.0 as usize].bounds;
                cx.draw.sprite(id, &fit(bounds, CELL - 8.0, x, y, 0.0), white, blend);
                if k == self.selected {
                    let h = CELL * 0.5 - 1.0;
                    let pts = [[x - h, y - h], [x + h, y - h], [x + h, y + h], [x - h, y + h], [x - h, y - h]];
                    cx.draw.shape(crate::gfx::Primitive::LineStrip, &Transform::IDENTITY, &pts, Argb(0xff40c0ff), Blend::Normal);
                }
            }
            if let Some(&id) = self.sprites.get(start + self.selected) {
                let bounds = cx.art.pack.sprites[id.0 as usize].bounds;
                let spin = (frame * 0.01).sin() * 0.6;
                cx.draw.sprite(id, &fit(bounds, 220.0, 520.0, 200.0, spin), white, blend);
                let name = cx.art.name(id).to_string();
                let hit = name
                    .rsplit_once('_')
                    .and_then(|(sheet, n)| Some((cx.assets.pat(sheet)?, n.parse::<usize>().ok()?)))
                    .and_then(|(pat, n)| pat.get(n).copied());
                let info = match hit {
                    Some(p) => format!("{}  HIT {}X{}", name.to_uppercase(), p.hit_half[0] * 2, p.hit_half[1] * 2),
                    None => name.to_uppercase(),
                };
                text::print(cx, |f: &Fonts| &f.small_text, 400.0, 330.0, &info, white);
            }
        }
        let header = format!(
            "PAGE {}/{}  {}  SOUND: {}",
            self.page + 1,
            self.pages(),
            if self.additive { "ADDITIVE" } else { "NORMAL" },
            if self.last_sound.is_empty() { "-".to_string() } else { self.last_sound.to_uppercase() }
        );
        text::print(cx, |f: &Fonts| &f.small_text, 16.0, 16.0, &header, Argb(0xffaaaaaa));
        text::print(
            cx,
            |f: &Fonts| &f.small_text,
            16.0,
            460.0,
            "LEFT/RIGHT PAGE  UP/DOWN SELECT  Z SOUND  X BLEND  ESC QUIT",
            Argb(0xff808080),
        );
    }
}
