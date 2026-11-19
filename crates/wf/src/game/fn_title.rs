//! fnTitle (fn_title.cpp, vtable 0x453474): the title screen.

use super::Cx;
use super::actor::{self, ActorDesc, Aid, Obj, sheet};
use super::app::Stage;
use super::background::BackgroundGrid;
use super::fn_game::{self, FnGame, FnGameState};
use super::fn_option::HEXAGON;
use super::input;
use super::math::sin;
use super::records;
use super::sounds;
use super::task::{self, ListId, Place, TaskId};
use super::text::{FontId, ObjText};
use crate::gfx::{Argb, Blend, Primitive, Transform};

const FLAG_MENU_CURSOR: u32 = 0x1;
const FLAG_TITLE_SHOWN: u32 = 0x2;

const STEP_LOGO: i16 = 100;
const STEP_INTRO: i16 = 200;
const STEP_TITLE: i16 = 300;
const STEP_MENU: i16 = 301;
const STEP_DEMO: i16 = 400;
const STEP_SELECTED: i16 = 500;

const MENU_START: i16 = 0;
const MENU_REPLAY: i16 = 1;
const MENU_OPTION: i16 = 2;
const MENU_EXIT: i16 = 3;
const MENU_ITEM_COUNT: i16 = 4;

const TITLE: &[u8] = b"WARNING   FOREVER"; // 0x45dbbc
const MENU_ITEMS: [&str; 4] = ["GAME_START", "  REPLAY", "  OPTION", "   EXIT"]; // 0x45dbd0
/// The arrows above and below the menu item (0x45dbe8).
const MENU_CURSOR: [[f32; 2]; 8] =
    [[0.0, -24.0], [-8.0, -16.0], [0.0, -24.0], [8.0, -16.0], [0.0, 24.0], [-8.0, 16.0], [0.0, 24.0], [8.0, 16.0]];

const SKIP_KEYS: u16 = input::SHOT | input::SLOW | input::WEAPON_SELECT;

#[derive(Default)]
pub struct FnTitle {
    logo: [Aid; 2],
    sweeps: [Aid; 2],
    subtitle: Aid,
    menu_text: Vec<u8>,
    scores: Vec<u8>,
    sweep_offset: f32,
    hex_scale: f32,
    flags: u32,
    cursor_alpha: i16,
    logo_alpha: i16,
    title_shown: i16,
    title_length: i16,
    scores_shown: i16,
    scores_length: i16,
    menu: i16,
    menu_x: i16,
    time: i16,
    step_time: i16,
    background: Option<TaskId>,
}

impl FnTitle {
    pub fn new() -> Self {
        Self::default()
    }

    fn subtitle_text(cx: &mut Cx) -> &mut Vec<u8> {
        &mut cx.obj_texts[0].text
    }

    fn set_x(cx: &mut Cx, a: Aid, x: f32) {
        let y = cx.actors.get(a).y();
        cx.actors.get_mut(a).set_position(x, y);
    }

    fn set_y(cx: &mut Cx, a: Aid, y: f32) {
        let x = cx.actors.get(a).x();
        cx.actors.get_mut(a).set_position(x, y);
    }

    fn set_logo_color(&self, cx: &mut Cx, color: u32) {
        cx.actors.get_mut(self.logo[1]).color = color;
        cx.actors.get_mut(self.logo[0]).color = color;
    }
}

impl Stage for FnTitle {
    /// Initialize (0x41e050).
    fn initialize(&mut self, cx: &mut Cx) -> bool {
        fn_game::clear_stage(cx);
        self.background = task::add(cx, ListId::Tasks, Box::new(BackgroundGrid::new(0xff00_4030, 8.0)), 2, Place::Back);
        cx.obj_texts = vec![ObjText { font: FontId::Text, text: Vec::new() }];
        let (scores, len) = cx.scores.format();
        self.scores = scores;
        self.scores_length = len as i16;
        self.title_length = TITLE.len() as i16;

        let mut desc = ActorDesc { hit_points: 1, y: 240.0, layer: 4, color: 0xffff_ffff, x: 800.0, ..Default::default() };
        desc.sprite = cx.pats.get(sheet::ETC0, 1);
        let s0 = actor::spawn(cx, &desc);
        desc.x = -160.0;
        desc.sprite = cx.pats.get(sheet::ETC0, 2);
        let s1 = actor::spawn(cx, &desc);
        desc.color = 0xffff_d000;
        desc.x = 184.0;
        desc.y = 304.0;
        desc.sprite = Obj::Text(0);
        let sub = actor::spawn(cx, &desc);

        let mut desc = ActorDesc { hit_points: 1, layer: 4, x: 320.0, y: 220.0, color: 0x00ff_ffff, ..Default::default() };
        desc.sprite = cx.pats.get(sheet::ETC0 + 2, 1);
        let l0 = actor::spawn(cx, &desc);
        desc.sprite = cx.pats.get(sheet::ETC0 + 2, 2);
        let l1 = actor::spawn(cx, &desc);
        let (Some(s0), Some(s1), Some(sub), Some(l0), Some(l1)) = (s0, s1, sub, l0, l1) else {
            crate::log::error("the title's actors could not be created");
            return false;
        };
        self.sweeps = [s0, s1];
        self.subtitle = sub;
        self.logo = [l0, l1];

        self.hex_scale = 0.0;
        self.time = STEP_LOGO;
        self.step_time = 0;
        self.menu = 0;
        self.title_shown = 0;
        self.scores_shown = 0;
        self.menu_x = -240;
        self.cursor_alpha = 0;
        self.logo_alpha = 0;
        self.flags = 0;
        self.menu_text = MENU_ITEMS[0].as_bytes().to_vec();
        cx.fade.fade_in(120);
        true
    }

    /// Finalize (0x41df70).
    fn finalize(&mut self, cx: &mut Cx) {
        actor::destroy_all(cx);
        cx.obj_texts.clear();
        if let Some(id) = self.background.take() {
            task::delete(cx, ListId::Tasks, id);
        }
    }

    /// Draw (0x41eae0).
    fn draw(&mut self, cx: &mut Cx) {
        if self.hex_scale > 0.0 {
            let mut t = Transform::IDENTITY;
            t.scale(self.hex_scale, self.hex_scale);
            let mut t = super::background::rotate(&t, cx.app.frame_count.wrapping_mul(3) as i32);
            t.m[2][0] += 300.0;
            t.m[2][1] += 320.0;
            cx.draw.shape(Primitive::LineStrip, &t, &HEXAGON, Argb(0x8000_ff80), Blend::Normal);
        }
        if self.flags & FLAG_MENU_CURSOR != 0 {
            let t = Transform::translation(128.0, 408.0);
            cx.draw.shape(Primitive::LineList, &t, &MENU_CURSOR, Argb(((self.cursor_alpha as u32) << 24) | 0xffdd00), Blend::Normal);
        }
        cx.fonts.text.print(&mut cx.draw, (self.menu_x + 48) as f32, 400.0, &self.menu_text, None, Argb(0x80ff_ffff), -1, Blend::Normal);
        if self.time >= 301 {
            let alpha = (-1i32).wrapping_sub(cx.app.frame_count.wrapping_mul(4) as i32) & 0xff;
            let mut palette = [Argb(0); 10];
            palette[0] = Argb(0x8000_ff80);
            palette[1] = Argb(((((((alpha >> 1) - 0x80) << 8) | alpha) << 16) as u32) | 0xff80);
            cx.fonts.small_text.print(&mut cx.draw, 32.0, 48.0, &self.scores, Some(&palette), palette[0], self.scores_shown as i32, Blend::Normal);
        }
        actor::draw_all(cx);
    }

    /// Move (0x41e300).
    fn update(&mut self, cx: &mut Cx) {
        self.step_time += 1;
        let time = self.time;
        let fading = cx.fade.fade_frames() != 0;
        match self.time {
            STEP_LOGO => {
                if !fading {
                    self.logo_alpha = 0;
                    self.time = time + 1;
                }
            }
            t if t == STEP_LOGO + 1 => {
                self.logo_alpha += 4;
                if self.logo_alpha >= 255 {
                    self.logo_alpha = 255;
                    self.time = time + 1;
                }
                self.set_logo_color(cx, ((self.logo_alpha as u32) << 24) | 0xff_ffff);
            }
            t if t == STEP_LOGO + 2 => {
                if self.step_time == 120 {
                    self.time = time + 1;
                }
            }
            t if t == STEP_LOGO + 3 => {
                self.logo_alpha -= 4;
                if self.logo_alpha <= 0 {
                    self.logo_alpha = 0;
                    self.time = time + 1;
                }
                self.set_logo_color(cx, ((self.logo_alpha as u32) << 24) | 0xff_ffff);
            }
            t if t == STEP_LOGO + 4 => {
                if self.step_time == 60 {
                    self.time = STEP_INTRO;
                }
            }
            STEP_INTRO => {
                if !fading {
                    self.sweep_offset = 480.0;
                    self.time = time + 1;
                }
            }
            t if t == STEP_INTRO + 1 => {
                self.sweep_offset *= 0.8;
                if self.sweep_offset < 1.0 {
                    self.sweep_offset = 0.0;
                    self.time = time + 1;
                }
                Self::set_x(cx, self.sweeps[0], self.sweep_offset + 320.0);
                Self::set_x(cx, self.sweeps[1], 320.0 - self.sweep_offset);
            }
            t if t == STEP_INTRO + 2 => {
                if self.step_time == 13 {
                    self.time = time + 1;
                }
                let (a, b) = (self.sweeps[0], self.sweeps[1]);
                Self::set_y(cx, a, cx.actors.get(a).y() - 4.0);
                Self::set_y(cx, b, cx.actors.get(b).y() + 4.0);
            }
            t if t == STEP_INTRO + 3 => {
                if self.step_time % 3 == 0 {
                    self.title_shown += 1;
                    let n = (self.title_shown as usize).min(TITLE.len());
                    *Self::subtitle_text(cx) = TITLE[..n].to_vec();
                    if self.title_shown >= self.title_length {
                        self.time += 1;
                    }
                }
            }
            t if t == STEP_INTRO + 4 => {
                if self.step_time == 8 {
                    self.time = time + 1;
                }
                for a in [self.sweeps[0], self.sweeps[1], self.subtitle] {
                    Self::set_y(cx, a, cx.actors.get(a).y() + 8.0);
                }
            }
            t if t == STEP_INTRO + 5 => {
                if self.step_time == 14 {
                    self.time = time + 1;
                }
                for a in [self.sweeps[0], self.sweeps[1], self.subtitle] {
                    Self::set_x(cx, a, cx.actors.get(a).x() + 11.0);
                }
            }
            t if t == STEP_INTRO + 6 => {
                self.hex_scale += 4.0;
                if self.hex_scale >= 65.0 {
                    self.hex_scale = 65.0;
                    self.time = STEP_TITLE;
                }
            }
            STEP_TITLE => {
                cx.actors.get_mut(self.sweeps[0]).set_position(474.0, 252.0);
                cx.actors.get_mut(self.sweeps[1]).set_position(474.0, 356.0);
                cx.actors.get_mut(self.subtitle).set_position(338.0, 368.0);
                self.set_logo_color(cx, 0xff_ffff);
                *Self::subtitle_text(cx) = TITLE.to_vec();
                self.hex_scale = 65.0;
                cx.fade.set_color(0, 0, 0, 0);
                self.flags |= FLAG_TITLE_SHOWN;
                self.time += 1;
            }
            STEP_MENU | 302 => {
                if self.time == STEP_MENU {
                    if self.menu_x != 0 {
                        let a = ((self.step_time as i32) << 8) / 15;
                        self.menu_x = super::math::ftos((1.0 - sin(a)) * -240.0);
                    } else {
                        self.flags |= FLAG_MENU_CURSOR;
                        self.time = time + 1;
                    }
                }
                self.menu_input(cx);
            }
            STEP_DEMO | STEP_SELECTED => {
                cx.fade.fade_to(60, 0, 0, 0, 255);
                self.time += 1;
            }
            t if t == STEP_DEMO + 1 => {
                if !fading {
                    match records::next_replay(cx) {
                        Some(path) => {
                            FnGameState::set_mode(cx, 2, Some(&path));
                            cx.app.finish(Some(Box::new(FnGame::new())));
                        }
                        None => cx.app.finish(Some(Box::new(FnTitle::new()))),
                    }
                }
            }
            t if t == STEP_SELECTED + 1 => {
                if !fading {
                    match self.menu {
                        MENU_REPLAY => cx.app.finish(Some(Box::new(FnGame::new()))),
                        MENU_OPTION => cx.app.finish(Some(Box::new(super::fn_option::FnOption::new()))),
                        MENU_EXIT => cx.app.finish(None),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        if self.flags & FLAG_TITLE_SHOWN == 0 && cx.input.pad(0).pressed & SKIP_KEYS != 0 {
            self.menu_x = 0;
            self.time = STEP_TITLE;
        }
        if self.flags & FLAG_MENU_CURSOR != 0 {
            self.cursor_alpha -= 4;
            if self.cursor_alpha <= 0 {
                self.cursor_alpha = 255;
            }
            if self.scores_shown < self.scores_length && cx.app.frame_count & 1 != 0 {
                self.scores_shown += 1;
            }
        }
        if time != self.time {
            self.step_time = 0;
        }
        actor::move_all(cx);
    }
}

impl FnTitle {
    /// The menu (steps 301 and 302).
    fn menu_input(&mut self, cx: &mut Cx) {
        let pressed = cx.input.pad(0).pressed;
        if pressed & SKIP_KEYS != 0 {
            match self.menu {
                MENU_START => {
                    FnGameState::set_mode(cx, 0, None);
                    cx.app.finish(Some(Box::new(FnGame::new())));
                }
                MENU_REPLAY => {
                    records::clear_replay(cx);
                    if let Some(path) = records::next_replay(cx) {
                        FnGameState::set_mode(cx, 1, Some(&path));
                        self.time = STEP_SELECTED;
                    }
                }
                _ => self.time = STEP_SELECTED,
            }
            cx.play_sound(sounds::SWITCH);
            return;
        }
        if pressed & (input::UP | input::DOWN) != 0 {
            if pressed & input::DOWN != 0 {
                self.menu += 1;
                if self.menu >= MENU_ITEM_COUNT {
                    self.menu = 0;
                }
            } else {
                self.menu -= 1;
                if self.menu < 0 {
                    self.menu = MENU_ITEM_COUNT - 1;
                }
            }
            self.menu_text = MENU_ITEMS[self.menu as usize].as_bytes().to_vec();
            self.cursor_alpha = 255;
            cx.play_sound(sounds::FIRE_ME);
            if self.step_time > 480 {
                self.step_time = 480;
            }
        }
        if self.step_time >= 960 {
            self.time = STEP_DEMO;
        }
    }
}
