//! fnOption (fn_option.cpp, g_fn_option): the option screen, and the
//! [Option] settings it reads and writes.

use super::Cx;
use super::app::Stage;
use super::background::BackgroundGrid;
use super::game_state::{FLAG_ENGLISH, FLAG_PAUSE, FLAG_STAGE_REPLAYS};
use super::input::{self, Pad};
use super::sounds;
use super::task::{self, ListId, Place, TaskId};
use super::texels;
use crate::gfx::{Argb, Blend, Primitive, Transform};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub enum Item {
    BossName,
    BgmVolume,
    SeVolume,
    BgmMode,
    ScoreReset,
    StageReplay,
    Dialog,
    GameMode,
    StartTime,
    TimerMode,
    Ships,
    Extend,
    ShotButton,
    SlowButton,
    WeaponButton,
    Pause,
    PauseButton,
    Exit,
}
pub const ITEMS: usize = 18;
const CUSTOM_PRESET: usize = 4;
const RULES: usize = 4;
const FIRST_RULE_ITEM: usize = Item::StartTime as usize;
const ITEM_GAME_MODE: usize = Item::GameMode as usize;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Choice,
    Formatted,
    Action,
    Button,
}

struct OptionItem {
    kind: Kind,
    count: i16,
    column: i16,
    /// Pixels per value (choices) or the trigger (buttons).
    param: u32,
    text: &'static str,
}

/// The menu (0x45da50); the cursor widths are in `OptionState::widths`.
const OPTION_ITEMS: [OptionItem; ITEMS] = [
    OptionItem { kind: Kind::Choice, count: 2, column: 0, param: 72, text: "BOSS NAME       : JAPANESE ENGLISH " },
    OptionItem { kind: Kind::Choice, count: 10, column: 32, param: 8, text: "BGM VOLUME      : MIN ---------- MAX" },
    OptionItem { kind: Kind::Choice, count: 10, column: 32, param: 8, text: "SE VOLUME       : MIN ---------- MAX" },
    OptionItem { kind: Kind::Choice, count: 3, column: 0, param: 48, text: "BGM MODE        : LOOP  FADE  REW " },
    OptionItem { kind: Kind::Choice, count: 2, column: 0, param: 48, text: "SCORE RESET     : NO    YES " },
    OptionItem { kind: Kind::Choice, count: 2, column: 0, param: 48, text: "STAGE REPLAY    : NO    YES " },
    OptionItem { kind: Kind::Choice, count: 2, column: 0, param: 48, text: "SETUP DIALOGBOX : HIDE  SHOW" },
    OptionItem { kind: Kind::Formatted, count: 5, column: 16, param: 0, text: "GAME MODE       : " },
    OptionItem { kind: Kind::Formatted, count: 10, column: 16, param: 0, text: "START TIME      : " },
    OptionItem { kind: Kind::Formatted, count: 2, column: 16, param: 0, text: "TIMER MODE      : " },
    OptionItem { kind: Kind::Formatted, count: 10, column: 16, param: 0, text: "NUMBER OF SHIPS : " },
    OptionItem { kind: Kind::Formatted, count: 3, column: 16, param: 0, text: "SHIP EXTEND     : " },
    OptionItem { kind: Kind::Button, count: 0, column: 25, param: 0, text: "SHOT            : BUTTON   " },
    OptionItem { kind: Kind::Button, count: 0, column: 25, param: 1, text: "SLOW MOVE       : BUTTON   " },
    OptionItem { kind: Kind::Button, count: 0, column: 25, param: 3, text: "WEAPON SELECT   : BUTTON   " },
    OptionItem { kind: Kind::Choice, count: 2, column: 0, param: 72, text: "GAME PAUSE      : DISABLE  ENABLE" },
    OptionItem { kind: Kind::Button, count: 0, column: 25, param: 7, text: "PAUSE BUTTON    : BUTTON   " },
    OptionItem { kind: Kind::Action, count: 0, column: 0, param: 0, text: "EXIT" },
];
/// The initial cursor widths (0x45da50's `width` fields).
const WIDTHS: [i16; ITEMS] = [64, 8, 8, 32, 32, 32, 32, 8, 8, 8, 8, 8, 0, 0, 0, 56, 0, 0];

/// Seconds of each START TIME, 0: none (0x45da00).
const START_TIMES: [i16; 10] = [0, 10, 30, 60, 120, 180, 240, 300, 600, 999];
const MODE_NAMES: [&str; 5] = ["NORMAL", "3 SHIPS", "SUDDEN DEATH", "5 MINUTES ATTACK", "CUSTOM"];
const TIMER_MODE_NAMES: [&str; 2] = ["NORMAL", "COUNT DOWN ONLY"];
const EXTEND_NAMES: [&str; 3] = ["NO EXTEND", "BY STAGE CLEAR", "BY 100 PARTS"];

/// The bar under the menu (font2 strip 1, "OPTION: OPTION:").
const TITLE_BAR: [[f32; 2]; 4] = [[0.0, -8.0], [768.0, -8.0], [0.0, 8.0], [768.0, 8.0]];
const TITLE_BAR_TEXELS: [[i16; 2]; 4] = [[0, 16], [768, 16], [0, 32], [768, 32]];

/// The volume of each step of the volume items.
const VOLUME_STEP: f32 = 1.0 / 9.0;

/// g_hexagon (0x45d0a0): the corners of the unit hexagon, closed.
pub const HEXAGON: [[f32; 2]; 7] =
    [[0.0, -2.0], [1.732_051, -1.0], [1.732_051, 1.0], [0.0, 2.0], [-1.732_051, 1.0], [-1.732_051, -1.0], [0.0, -2.0]];

/// The fields of g_fn_option that outlive the screen.
#[derive(Clone, Debug)]
pub struct OptionState {
    pub values: [i16; ITEMS],
    pub enabled: [bool; ITEMS],
    /// presets_ (0x45d9b0): NORMAL, 3 SHIPS, SUDDEN DEATH, 5 MINUTES
    /// ATTACK, CUSTOM; the last one is edited.
    pub presets: [[i32; RULES]; 5],
    widths: [i16; ITEMS],
}

impl Default for OptionState {
    fn default() -> Self {
        OptionState {
            values: [0; ITEMS],
            enabled: [true; ITEMS],
            presets: [[5, 0, 0, 0], [0, 0, 3, 2], [0, 0, 1, 0], [7, 1, 0, 0], [5, 0, 0, 0]],
            widths: WIDTHS,
        }
    }
}

impl OptionState {
    /// SetPreset (0x41c8a0).
    fn set_preset(&mut self, preset: usize) {
        let rules = self.presets[preset];
        let custom = preset == CUSTOM_PRESET;
        for i in 0..RULES {
            self.enabled[FIRST_RULE_ITEM + i] = custom;
            self.values[FIRST_RULE_ITEM + i] = rules[i] as i16;
        }
    }
}

fn trigger(cx: &Cx, item: usize) -> i16 {
    cx.input.pad(0).trigger(OPTION_ITEMS[item].param as usize) as i16
}

/// cGlobal::SetEffectVolume (0x41c6a0), SetMusicVolume (0x41c6c0).
pub fn set_effect_volume(cx: &mut Cx, volume: f32) {
    cx.global.effect_volume = volume;
    cx.sound.set_group_volume(volume, Some(0));
}
pub fn set_music_volume(cx: &mut Cx, volume: f32) {
    cx.global.music_volume = volume;
    cx.sound.set_group_volume(volume, Some(1));
}

/// Apply (0x41c730): the options to the game.
pub fn apply(cx: &mut Cx) {
    let v = cx.options.values;
    set_music_volume(cx, v[Item::BgmVolume as usize] as f32 * VOLUME_STEP);
    set_effect_volume(cx, v[Item::SeVolume as usize] as f32 * VOLUME_STEP);
    cx.fn_game.bgm_mode = v[Item::BgmMode as usize] as i32;
    let flag = |cx: &mut Cx, f: u32, on: bool| {
        if on {
            cx.global.flags |= f;
        } else {
            cx.global.flags &= !f;
        }
    };
    flag(cx, FLAG_STAGE_REPLAYS, v[Item::StageReplay as usize] != 0);
    flag(cx, FLAG_PAUSE, v[Item::Pause as usize] != 0);
    flag(cx, FLAG_ENGLISH, v[Item::BossName as usize] != 0);
    let r = &mut cx.global.rules;
    r.time_limit = START_TIMES[v[Item::StartTime as usize] as usize];
    r.stock = v[Item::Ships as usize];
    match v[Item::TimerMode as usize] {
        0 => r.mode = 0,
        1 => r.mode = 1,
        _ => {}
    }
    if let 0..=2 = v[Item::Extend as usize] {
        r.extend = v[Item::Extend as usize];
    }
    let rules = cx.global.rules;
    cx.scores.set_rules(&rules);
    if !cx.scores.load() {
        cx.scores.reset();
    }
    if cx.options.values[Item::ScoreReset as usize] != 0 {
        cx.scores.reset();
        cx.scores.store();
        cx.options.values[Item::ScoreReset as usize] = 0;
    }
    cx.scores.clear_rank();
}

/// LoadSettings (0x41caf0). The boss names default to English unless the
/// system language is Japanese.
pub fn load_settings(cx: &mut Cx) {
    let japanese = sys_locale_is_japanese();
    let p = &mut cx.profile;
    p.set_section("Option");
    let o = &mut cx.options;
    o.values[Item::BgmVolume as usize] = p.get_int("BGMVolume", 8) as i16;
    o.values[Item::SeVolume as usize] = p.get_int("SEVolume", 8) as i16;
    o.values[Item::BgmMode as usize] = p.get_int("BGMMode", 0) as i16;
    o.values[Item::StageReplay as usize] = p.get_bool("RecAllStage", false) as i16;
    o.values[Item::GameMode as usize] = p.get_int("GameMode", 0) as i16;
    o.values[Item::Pause as usize] = p.get_bool("AppPause", false) as i16;
    o.values[Item::BossName as usize] = p.get_bool("NameEnglish", !japanese) as i16;
    let c = &mut o.presets[CUSTOM_PRESET];
    c[0] = p.get_int("CustomStartTimer", c[0]);
    c[1] = p.get_int("CustomTimerMode", c[1]);
    c[2] = p.get_int("CustomStartPlayer", c[2]);
    c[3] = p.get_int("CustomPlayerExtend", c[3]);
    if c[0] == 0 && c[2] == 0 {
        c[2] = 1;
    }
    // Out-of-range values in a hand-edited file are clamped (the original
    // indexes its tables with them).
    for (i, item) in OPTION_ITEMS.iter().enumerate() {
        if item.count > 0 {
            o.values[i] = o.values[i].clamp(0, item.count - 1);
        }
    }
    for (i, v) in c.iter_mut().enumerate() {
        *v = (*v).clamp(0, OPTION_ITEMS[FIRST_RULE_ITEM + i].count as i32 - 1);
    }
    let preset = o.presets[o.values[ITEM_GAME_MODE] as usize];
    for i in 0..RULES {
        o.values[FIRST_RULE_ITEM + i] = preset[i] as i16;
    }
    p.set_section("DirectX");
    o.values[Item::Dialog as usize] = p.get_bool("ViewDialog", true) as i16;
    // The trigger maps (HLInpDev::LoadTriggerMap).
    p.set_section("TriggerMap");
    let keys = p.get_string("Keyboard", "");
    let buttons = p.get_string("Gamepad", "");
    cx.input.pads[0].set_trigger_maps(&keys, &buttons);
    let o = &mut cx.options;
    o.values[Item::ScoreReset as usize] = 0;
    o.values[Item::Exit as usize] = 0;
    for item in [Item::ShotButton, Item::SlowButton, Item::WeaponButton, Item::PauseButton] {
        let t = trigger(cx, item as usize);
        cx.options.values[item as usize] = t;
    }
    apply(cx);
}

/// SaveSettings (0x41cfc0).
pub fn save_settings(cx: &mut Cx) {
    let v = cx.options.values;
    let c = cx.options.presets[CUSTOM_PRESET];
    let p = &mut cx.profile;
    p.set_section("Option");
    p.write_int("BGMVolume", v[Item::BgmVolume as usize] as i32);
    p.write_int("SEVolume", v[Item::SeVolume as usize] as i32);
    p.write_int("BGMMode", v[Item::BgmMode as usize] as i32);
    p.write_bool("RecAllStage", v[Item::StageReplay as usize] != 0);
    p.write_bool("AppPause", v[Item::Pause as usize] != 0);
    p.write_bool("NameEnglish", v[Item::BossName as usize] != 0);
    p.write_int("GameMode", v[ITEM_GAME_MODE] as i32);
    p.write_int("CustomStartTimer", c[0]);
    p.write_int("CustomTimerMode", c[1]);
    p.write_int("CustomStartPlayer", c[2]);
    p.write_int("CustomPlayerExtend", c[3]);
    p.set_section("DirectX");
    p.write_bool("ViewDialog", v[Item::Dialog as usize] != 0);
    let (keys, buttons) = cx.input.pad(0).trigger_maps();
    p.set_section("TriggerMap");
    p.write_string("Keyboard", &keys);
    p.write_string("Gamepad", &buttons);
}

/// GetUserDefaultLangID() == 0x411, through the locale environment.
fn sys_locale_is_japanese() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .find(|v| !v.is_empty())
        .is_some_and(|v| v.starts_with("ja"))
}

/// The option screen (FnOption as a stage).
pub struct FnOption {
    flags: u32,
    cursor: i16,
    blink: i16,
    items: Vec<Vec<u8>>,
    background: Option<TaskId>,
}

const FLAG_EXIT: u32 = 0x1;
const FLAG_HELD: u32 = 0x2;
const FLAG_MUSIC: u32 = 0x4;

impl FnOption {
    pub fn new() -> Self {
        FnOption { flags: 0, cursor: 0, blink: 0, items: vec![Vec::new(); ITEMS], background: None }
    }

    /// UpdateButtonText (0x41d340).
    fn update_button_text(&mut self, cx: &Cx, item: usize) {
        let name = Pad::name(cx.options.values[item] as i32);
        let col = OPTION_ITEMS[item].column as usize;
        let t = &mut self.items[item];
        if t.len() < col + 2 {
            t.resize(col + 2, b' ');
        }
        t[col..col + 2].copy_from_slice(&name);
    }

    /// UpdateChoiceText (0x41d390).
    fn update_choice_text(&mut self, cx: &mut Cx, item: usize) {
        let value = cx.options.values[item];
        let name: String = match item {
            ITEM_GAME_MODE => MODE_NAMES[value as usize].into(),
            x if x == Item::StartTime as usize && value != 0 => format!("{} SECONDS", START_TIMES[value as usize]),
            x if x == Item::TimerMode as usize => TIMER_MODE_NAMES[value as usize].into(),
            x if x == Item::Ships as usize && value >= 2 => format!("{value} SHIPS"),
            x if x == Item::Ships as usize && value != 0 => "1 SHIP".into(),
            x if x == Item::Extend as usize => EXTEND_NAMES[value as usize].into(),
            _ => "INFINITY".into(),
        };
        let (left, right) = if cx.options.enabled[item] {
            (if value != 0 { "< " } else { "  " }, if value != OPTION_ITEMS[item].count - 1 { " >" } else { "" })
        } else {
            ("  ", "")
        };
        self.items[item] = format!("{}{left}{name}{right}", OPTION_ITEMS[item].text).into_bytes();
        cx.options.widths[item] = (name.len() * 8) as i16;
    }

    fn play(cx: &mut Cx, id: usize) {
        cx.play_sound(id);
    }
}

impl Default for FnOption {
    fn default() -> Self {
        Self::new()
    }
}

impl Stage for FnOption {
    /// Initialize (0x41dc60).
    fn initialize(&mut self, cx: &mut Cx) -> bool {
        super::fn_game::clear_stage(cx);
        self.background = task::add(cx, ListId::Tasks, Box::new(BackgroundGrid::new(0xff00_4030, 8.0)), 2, Place::Back);
        let preset = cx.options.values[ITEM_GAME_MODE] as usize;
        cx.options.set_preset(preset);
        for i in 0..ITEMS {
            if OPTION_ITEMS[i].kind == Kind::Formatted {
                self.update_choice_text(cx, i);
            } else {
                self.items[i] = OPTION_ITEMS[i].text.as_bytes().to_vec();
            }
        }
        self.cursor = Item::Exit as i16;
        self.flags = 0;
        self.blink = 0;
        for item in [Item::ShotButton, Item::SlowButton, Item::WeaponButton, Item::PauseButton] {
            self.update_button_text(cx, item as usize);
        }
        cx.fade.set_color(0, 0, 0, 255);
        cx.fade.fade_to(60, 0, 0, 0, 0);
        true
    }

    /// Finalize (0x41ded0).
    fn finalize(&mut self, cx: &mut Cx) {
        apply(cx);
        save_settings(cx);
        if let Some(id) = self.background.take() {
            task::delete(cx, ListId::Tasks, id);
        }
    }

    /// Move (0x41d4d0).
    fn update(&mut self, cx: &mut Cx) {
        self.blink -= 4;
        if self.blink <= 0 {
            self.blink = 255;
        }
        if cx.fade.fade_frames() != 0 {
            return;
        }
        if self.flags & FLAG_EXIT != 0 {
            cx.app.finish(Some(Box::new(super::fn_title::FnTitle::new())));
            return;
        }
        let pressed = cx.input.pad(0).pressed;
        if pressed & (input::UP | input::DOWN) != 0 {
            loop {
                if pressed & input::UP != 0 {
                    self.cursor -= 1;
                    if self.cursor < 0 {
                        self.cursor = ITEMS as i16 - 1;
                    }
                } else {
                    self.cursor += 1;
                    if self.cursor >= ITEMS as i16 {
                        self.cursor = 0;
                    }
                }
                if cx.options.enabled[self.cursor as usize] {
                    break;
                }
            }
            Self::play(cx, sounds::FIRE_ME);
            self.blink = 255;
            self.flags |= FLAG_HELD;
            return;
        }
        let c = self.cursor as usize;
        match OPTION_ITEMS[c].kind {
            Kind::Choice | Kind::Formatted => {
                let old = cx.options.values[c];
                let v = &mut cx.options.values[c];
                if pressed & input::LEFT != 0 {
                    *v -= 1;
                    if *v < 0 {
                        *v = 0;
                    }
                } else if pressed & input::RIGHT != 0 {
                    *v += 1;
                    if *v >= OPTION_ITEMS[c].count {
                        *v = OPTION_ITEMS[c].count - 1;
                    }
                }
                let value = *v;
                if old == value {
                    return;
                }
                match c {
                    x if x == Item::BgmVolume as usize => {
                        if self.flags & FLAG_MUSIC == 0 && cx.assets.bgm.is_some() {
                            cx.play_bgm();
                            self.flags |= FLAG_MUSIC;
                        }
                        set_music_volume(cx, value as f32 * VOLUME_STEP);
                    }
                    x if x == Item::SeVolume as usize => set_effect_volume(cx, value as f32 * VOLUME_STEP),
                    ITEM_GAME_MODE => cx.options.set_preset(value as usize),
                    x if x == Item::StartTime as usize => {
                        if value == 0 && cx.options.values[Item::Ships as usize] == 0 {
                            cx.options.values[Item::Ships as usize] = 1;
                        }
                    }
                    x if x == Item::Ships as usize => {
                        if value == 0 && cx.options.values[Item::StartTime as usize] == 0 {
                            cx.options.values[Item::StartTime as usize] = 1;
                        }
                    }
                    _ => {}
                }
                if OPTION_ITEMS[c].kind == Kind::Formatted {
                    self.update_choice_text(cx, c);
                    let mode = cx.options.values[ITEM_GAME_MODE] as usize;
                    for i in 0..RULES {
                        cx.options.presets[mode][i] = cx.options.values[FIRST_RULE_ITEM + i] as i32;
                        self.update_choice_text(cx, FIRST_RULE_ITEM + i);
                    }
                }
                Self::play(cx, sounds::SWITCH);
            }
            Kind::Action => {
                if pressed & (input::TRIGGER * 0xb) != 0 {
                    if c == Item::Exit as usize {
                        cx.sound.fade(super::assets::CHANNEL_BGM, 1.0, 0.0, 1.0, true);
                        cx.fade.fade_out(60);
                        self.flags |= FLAG_EXIT;
                    }
                    Self::play(cx, sounds::SWITCH);
                }
            }
            Kind::Button => {
                let Some(code) = cx.input.get_pressed() else {
                    self.flags &= !FLAG_HELD;
                    return;
                };
                if self.flags & FLAG_HELD != 0 {
                    return;
                }
                self.flags |= FLAG_HELD;
                Self::play(cx, sounds::SWITCH);
                // A gamepad button is assigned to the gamepads only; the
                // item keeps showing it until the screen is left.
                if code as i16 != cx.options.values[c] {
                    cx.input.pads[0].set_trigger(OPTION_ITEMS[c].param as usize, code);
                    cx.options.values[c] = code as i16;
                    self.update_button_text(cx, c);
                }
            }
        }
    }

    /// Draw (0x41d880).
    fn draw(&mut self, cx: &mut Cx) {
        let mut t = Transform::IDENTITY;
        t.scale(65.0, 65.0);
        let t = super::background::rotate(&t, cx.app.frame_count.wrapping_mul(3) as i32);
        let mut t = t;
        t.translate(300.0, 320.0);
        cx.draw.shape(Primitive::LineStrip, &t, &HEXAGON, Argb(0x6400_ff80), Blend::Normal);

        let id = Transform::IDENTITY;
        let mut y = 140.0f32;
        for i in 0..ITEMS {
            let item = &OPTION_ITEMS[i];
            if item.count != 0 && cx.options.enabled[i] {
                let x0 = (cx.options.values[i] as i32 * item.param as i32 + item.column as i32 + 464) as f32;
                let x1 = cx.options.widths[i] as f32 + x0;
                let quad = [[x0, y], [x1, y], [x0, y + 8.0], [x1, y + 8.0]];
                cx.draw.shape(Primitive::TriangleStrip, &id, &quad, Argb(0x6400_ff80), Blend::Normal);
            }
            y += 16.0;
        }
        let c = self.cursor as usize;
        let len = self.items[c].len() as f32;
        let cy = c as f32 * 16.0 + 148.0;
        let line = [[320.0, cy], [len * 8.0 + 320.0, cy]];
        cx.draw.shape(Primitive::LineList, &id, &line, Argb(((self.blink as u32) << 24) | 0xffdd00), Blend::Normal);
        let mut y = 140.0f32;
        for text in &self.items {
            cx.fonts.small_text.print(&mut cx.draw, 320.0, y.trunc(), text, None, Argb(0x80ff_ffff), -1, Blend::Normal);
            y += 16.0;
        }
        let mut t = Transform::IDENTITY;
        t.m[2][0] = -(((cx.app.frame_count & 0x3f) * 2) as f32);
        t.m[2][1] = 450.0;
        texels::draw(cx, &texels::font2_strip(1), &t, &TITLE_BAR, &TITLE_BAR_TEXELS, 0xc800_ff80);
    }
}
