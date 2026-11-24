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
use super::settings::{self, TabBy};
use crate::gfx::{Argb, Blend, Primitive, Transform};
use wf_engine::platform::{Capabilities, Device, PadButton};
use wf_engine::time::FpsCap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub enum Item {
    BossName,
    BgmVolume,
    SeVolume,
    BgmMode,
    ScoreReset,
    StageReplay,
    GameMode,
    StartTime,
    TimerMode,
    Ships,
    Extend,
    Pause,
    // The port's own (wf-plus.ini).
    DefaultArena,
    WindowMode,
    AspectRatio,
    Resolution,
    RenderScale,
    FpsCap,
    ShowFps,
    HideNames,
    ShotKey,
    SlowKey,
    WeaponKey,
    PauseKey,
    MouseLeft,
    MouseMiddle,
    MouseRight,
    MouseSteering,
    DeadZone,
    TouchScale,
    TouchAim,
    // The gamepad's buttons of each action (settings::PAD_ACTIONS), three
    // slots each.
    PadShot1,
    PadShot2,
    PadShot3,
    PadSlow1,
    PadSlow2,
    PadSlow3,
    PadWeapon1,
    PadWeapon2,
    PadWeapon3,
    PadNames,
    Exit,
}
pub const ITEMS: usize = Item::Exit as usize + 1;
const CUSTOM_PRESET: usize = 4;
const RULES: usize = 4;
const FIRST_RULE_ITEM: usize = Item::StartTime as usize;
const ITEM_GAME_MODE: usize = Item::GameMode as usize;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Choice,
    Formatted,
    Action,
    /// A gamepad button slot to remap (param: action * PAD_SLOTS + slot;
    /// value: the button's index in `PadButton::BINDABLE` + 1, 0 none).
    Button,
    /// A keyboard key to remap (param: the trigger).
    Key,
}

struct OptionItem {
    kind: Kind,
    count: i16,
    column: i16,
    /// Pixels per value (choices) or the trigger (buttons).
    param: u32,
    text: &'static str,
}

const fn choice(count: i16, param: u32, text: &'static str) -> OptionItem {
    OptionItem { kind: Kind::Choice, count, column: 0, param, text }
}
const fn formatted(count: i16, text: &'static str) -> OptionItem {
    OptionItem { kind: Kind::Formatted, count, column: 16, param: 0, text }
}
const fn button(kind: Kind, trigger: u32, text: &'static str) -> OptionItem {
    OptionItem { kind, count: 0, column: 0, param: trigger, text }
}

/// The menu (0x45da50, without SETUP DIALOGBOX, plus the port's items);
/// the cursor widths are in `OptionState::widths`.
const OPTION_ITEMS: [OptionItem; ITEMS] = [
    choice(2, 72, "BOSS NAME       : JAPANESE ENGLISH "),
    OptionItem { kind: Kind::Choice, count: 10, column: 32, param: 8, text: "BGM VOLUME      : MIN ---------- MAX" },
    OptionItem { kind: Kind::Choice, count: 10, column: 32, param: 8, text: "SE VOLUME       : MIN ---------- MAX" },
    choice(3, 48, "BGM MODE        : LOOP  FADE  REW "),
    choice(2, 48, "SCORE RESET     : NO    YES "),
    choice(2, 48, "STAGE REPLAY    : NO    YES "),
    formatted(5, "GAME MODE       : "),
    formatted(10, "START TIME      : "),
    formatted(2, "TIMER MODE      : "),
    formatted(10, "NUMBER OF SHIPS : "),
    formatted(3, "SHIP EXTEND     : "),
    choice(2, 72, "GAME PAUSE      : DISABLE  ENABLE"),
    formatted(2, "DEFAULT ARENA   : "),
    formatted(3, "WINDOW MODE     : "),
    // The display's ratios and sizes (OptionState::aspects, sizes).
    formatted(1, "ASPECT RATIO    : "),
    formatted(1, "RESOLUTION      : "),
    formatted(settings::RENDER_SCALES.len() as i16, "RENDER SCALE    : "),
    // The presets, MAX (the display's refresh rate), CUSTOM.
    formatted(FpsCap::PRESETS.len() as i16 + 2, "FPS CAP         : "),
    // OFF, ON, ON (GPU MS) (the frame's GPU time too: ui.perf_overlay).
    formatted(3, "SHOW FPS        : "),
    formatted(2, "HIDE NAMES      : "),
    button(Kind::Key, 0, "SHOT      (KEY) : "),
    button(Kind::Key, 1, "SLOW MOVE (KEY) : "),
    button(Kind::Key, 3, "WEAPON    (KEY) : "),
    button(Kind::Key, 7, "PAUSE     (KEY) : "),
    formatted(settings::MOUSE_ACTIONS.len() as i16, "MOUSE LEFT      : "),
    formatted(settings::MOUSE_ACTIONS.len() as i16, "MOUSE MIDDLE    : "),
    formatted(settings::MOUSE_ACTIONS.len() as i16, "MOUSE RIGHT     : "),
    formatted(2, "MOUSE STEERING  : "),
    formatted(settings::DEAD_ZONES.len() as i16, "PAD DEADZONE    : "),
    formatted(settings::TOUCH_SCALES.len() as i16, "TOUCH SPEED     : "),
    formatted(settings::TOUCH_AIMS.len() as i16, "TOUCH AIM       : "),
    // param: action * PAD_SLOTS + slot.
    button(Kind::Button, 0, "SHOT 1          : "),
    button(Kind::Button, 1, "SHOT 2          : "),
    button(Kind::Button, 2, "SHOT 3          : "),
    button(Kind::Button, 3, "SLOW MOVE 1     : "),
    button(Kind::Button, 4, "SLOW MOVE 2     : "),
    button(Kind::Button, 5, "SLOW MOVE 3     : "),
    button(Kind::Button, 6, "WEAPON 1        : "),
    button(Kind::Button, 7, "WEAPON 2        : "),
    button(Kind::Button, 8, "WEAPON 3        : "),
    formatted(settings::PAD_NAMES.len() as i16, "BUTTON NAMES    : "),
    OptionItem { kind: Kind::Action, count: 0, column: 0, param: 0, text: "EXIT" },
];
/// The initial cursor widths (0x45da50's `width` fields).
const WIDTHS: [i16; ITEMS] = {
    let mut w = [8; ITEMS];
    w[Item::BossName as usize] = 64;
    w[Item::BgmMode as usize] = 32;
    w[Item::ScoreReset as usize] = 32;
    w[Item::StageReplay as usize] = 32;
    w[Item::Pause as usize] = 56;
    w
};

/// The tabs and their items, top to bottom (EXIT ends each).
const TABS: [(&str, &[Item]); 6] = {
    use Item::*;
    [
        ("GAME", &[GameMode, StartTime, TimerMode, Ships, Extend, Pause, DefaultArena, Exit]),
        ("DISPLAY", &[WindowMode, AspectRatio, Resolution, RenderScale, FpsCap, ShowFps, HideNames, BossName, Exit]),
        ("AUDIO", &[BgmVolume, SeVolume, BgmMode, Exit]),
        (
            "CONTROLS",
            &[
                ShotKey, SlowKey, WeaponKey, PauseKey, MouseLeft, MouseMiddle, MouseRight, MouseSteering, TouchScale, TouchAim, Exit,
            ],
        ),
        ("PAD", &[PadShot1, PadShot2, PadShot3, PadSlow1, PadSlow2, PadSlow3, PadWeapon1, PadWeapon2, PadWeapon3, DeadZone, PadNames, Exit]),
        ("RECORDS", &[StageReplay, ScoreReset, Exit]),
    ]
};
/// The tab strip: its top, its left edge, and the gap between the tabs.
const TAB_Y: f32 = 104.0;
const TAB_X0: f32 = 320.0;
const TAB_GAP: f32 = 16.0;

/// Whether the platform lets `item` be changed (the options omit the rest).
fn supported(item: Item, caps: &Capabilities) -> bool {
    match item {
        Item::WindowMode => caps.window_mode,
        Item::AspectRatio | Item::Resolution => caps.window_size,
        _ => true,
    }
}

/// The tabs with the items the platform supports; a tab left with only
/// EXIT is dropped.
fn build_tabs(caps: &Capabilities) -> Vec<(&'static str, Vec<Item>)> {
    TABS.iter()
        .map(|(name, items)| (*name, items.iter().copied().filter(|&i| supported(i, caps)).collect::<Vec<_>>()))
        .filter(|(_, items)| items.len() > 1)
        .collect()
}

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

/// The fields of g_fn_option that outlive the screen.
#[derive(Clone, Debug)]
pub struct OptionState {
    pub values: [i16; ITEMS],
    pub enabled: [bool; ITEMS],
    /// presets_ (0x45d9b0): NORMAL, 3 SHIPS, SUDDEN DEATH, 5 MINUTES
    /// ATTACK, CUSTOM; the last one is edited.
    pub presets: [[i32; RULES]; 5],
    widths: [i16; ITEMS],
    /// The tabs and their items, as the platform allows.
    tabs: Vec<(&'static str, Vec<Item>)>,
    /// The tab shown, and the cursor row of each tab (kept while the game
    /// runs).
    tab: usize,
    rows: [usize; 6],
    /// ASPECT RATIO's choices: the ratios of the display's sizes (and of
    /// the window's size, if it is none of them), narrowest first.
    aspects: Vec<(u32, u32)>,
    /// RESOLUTION's choices: the display's sizes of the chosen ratio (and
    /// the window's size), smallest first.
    sizes: Vec<(u32, u32)>,
}

impl Default for OptionState {
    fn default() -> Self {
        let tabs = build_tabs(&wf_engine::platform::capabilities());
        let mut rows = [0; TABS.len()];
        for (row, (_, items)) in rows.iter_mut().zip(&tabs) {
            *row = items.len() - 1;
        }
        OptionState {
            values: [0; ITEMS],
            enabled: [true; ITEMS],
            presets: [[5, 0, 0, 0], [0, 0, 3, 2], [0, 0, 1, 0], [7, 1, 0, 0], [5, 0, 0, 0]],
            widths: WIDTHS,
            tabs,
            tab: 0,
            rows,
            aspects: Vec::new(),
            sizes: Vec::new(),
        }
    }
}

impl OptionState {
    /// How many values `item` has: the display's for ASPECT RATIO and
    /// RESOLUTION.
    fn count(&self, item: usize) -> i16 {
        match item {
            x if x == Item::AspectRatio as usize => self.aspects.len().max(1) as i16,
            x if x == Item::Resolution as usize => self.sizes.len().max(1) as i16,
            _ => OPTION_ITEMS[item].count,
        }
    }

    /// Lists the display's ratios and the sizes of the window's ratio
    /// (`size`), and points ASPECT RATIO and RESOLUTION at it.
    fn list_sizes(&mut self, size: (u32, u32)) {
        let mut all = settings::resolutions();
        all.push(size);
        let ratio = settings::aspect(size.0, size.1);
        self.aspects = settings::aspects(&all);
        self.sizes = settings::sizes_of(&all, ratio);
        self.values[Item::AspectRatio as usize] = self.aspects.iter().position(|&r| r == ratio).unwrap_or(0) as i16;
        self.values[Item::Resolution as usize] = self.sizes.iter().position(|&r| r == size).unwrap_or(0) as i16;
    }

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

/// What a remap item shows: the trigger's key (Key items) or the slot's
/// button (Button items, see `Kind::Button`).
fn trigger(cx: &Cx, item: usize) -> i16 {
    let t = OPTION_ITEMS[item].param as usize;
    match OPTION_ITEMS[item].kind {
        Kind::Button => {
            let b = cx.settings.pad[t / settings::PAD_SLOTS][t % settings::PAD_SLOTS];
            b.and_then(|b| PadButton::BINDABLE.iter().position(|&x| x == b)).map_or(0, |i| i as i16 + 1)
        }
        _ => cx.input.pad(0).trigger(t) as i16,
    }
}

/// The keyboard remap items.
const REMAPS: [Item; 4] = [
    Item::ShotKey,
    Item::SlowKey,
    Item::WeaponKey,
    Item::PauseKey,
];

/// cGlobal::SetEffectVolume (0x41c6a0), SetMusicVolume (0x41c6c0).
pub fn set_effect_volume(cx: &mut Cx, volume: f32) {
    cx.global.effect_volume = volume;
    cx.sound.set_group_volume(volume, Some(sounds::SOUND_EFFECTS));
}
pub fn set_music_volume(cx: &mut Cx, volume: f32) {
    cx.global.music_volume = volume;
    cx.sound.set_group_volume(volume, Some(sounds::SOUND_MUSIC));
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
    let reset = cx.options.values[Item::ScoreReset as usize] != 0;
    // Both arena modes' tables follow the rules (and the score reset).
    for scores in [&mut cx.scores, &mut cx.scores_alt] {
        scores.set_rules(&rules);
        if !scores.load() {
            scores.reset();
        }
        if reset {
            scores.reset();
            scores.store();
        }
        scores.clear_rank();
    }
    cx.options.values[Item::ScoreReset as usize] = 0;
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
    // The trigger maps (HLInpDev::LoadTriggerMap).
    p.set_section("TriggerMap");
    let keys = p.get_string("Keyboard", "");
    let buttons = p.get_string("Gamepad", "");
    cx.input.pads[0].set_trigger_maps(&keys, &buttons);
    // Without the port's pad slots, a button wf.ini changed (an earlier
    // version's options, or the original's) replaces that action's
    // defaults.
    if !cx.settings.pad_custom {
        for (k, &(t, _)) in settings::PAD_ACTIONS.iter().enumerate() {
            let i = cx.input.pad(0).button_map[t as usize];
            if i != t
                && let Some(&b) = PadButton::TRIGGERS.get(i as usize)
                && PadButton::BINDABLE.contains(&b)
            {
                cx.settings.pad[k] = [Some(b), None, None];
                cx.settings.input_dirty = true;
            }
        }
    }
    let o = &mut cx.options;
    o.values[Item::ScoreReset as usize] = 0;
    o.values[Item::Exit as usize] = 0;
    for item in REMAPS {
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


/// The option screen (FnOption as a stage), as a tab strip: GAME, DISPLAY,
/// AUDIO, CONTROLS and RECORDS, each tab's items below it.
pub struct FnOption {
    flags: u32,
    blink: i16,
    items: Vec<Vec<u8>>,
    background: Option<TaskId>,
    /// The tab switch's slide: frames left, and which way (+1: to the
    /// next tab).
    slide: i16,
    slide_dir: f32,
    /// The gamepad family the pad rows name their buttons for.
    pad_style: wf_engine::platform::PadStyle,
}

const FLAG_EXIT: u32 = 0x1;
const FLAG_HELD: u32 = 0x2;
const FLAG_MUSIC: u32 = 0x4;
const SLIDE_FRAMES: i16 = 12;
/// The first item row's y, and the rows' spacing.
const ROW_Y: f32 = 140.0;
const ROW_H: f32 = 16.0;

impl FnOption {
    pub fn new() -> Self {
        FnOption {
            flags: 0,
            blink: 0,
            items: vec![Vec::new(); ITEMS],
            background: None,
            slide: 0,
            slide_dir: 1.0,
            pad_style: Default::default(),
        }
    }

    fn tab_items(cx: &Cx) -> Vec<Item> {
        cx.options.tabs[cx.options.tab].1.clone()
    }

    /// The tabs for the platform: the rows stay on the items they were on
    /// (else on EXIT).
    fn set_tabs(cx: &mut Cx) {
        let old = std::mem::replace(&mut cx.options.tabs, build_tabs(&cx.settings.caps));
        let o = &mut cx.options;
        let mut rows = [0; TABS.len()];
        for (t, (name, items)) in o.tabs.iter().enumerate() {
            let was = old.iter().position(|(n, _)| n == name).and_then(|i| old[i].1.get(o.rows[i]).copied());
            rows[t] = was.and_then(|w| items.iter().position(|&i| i == w)).unwrap_or(items.len() - 1);
        }
        let tab = old.get(o.tab).and_then(|(n, _)| o.tabs.iter().position(|(m, _)| m == n)).unwrap_or(0);
        o.rows = rows;
        o.tab = tab;
    }

    /// Each tab's left edge on the strip.
    fn tab_x(cx: &Cx, tab: usize) -> f32 {
        let gap = Self::tab_gap(cx);
        TAB_X0 + cx.options.tabs[..tab].iter().map(|(n, _)| n.len() as f32 * 8.0 + gap).sum::<f32>()
    }

    /// The gap between the tabs: `TAB_GAP`, less if the strip would pass
    /// the screen's right edge.
    fn tab_gap(cx: &Cx) -> f32 {
        let tabs = &cx.options.tabs;
        let text: f32 = tabs.iter().map(|(n, _)| n.len() as f32 * 8.0).sum();
        let gaps = tabs.len().saturating_sub(1).max(1) as f32;
        TAB_GAP.min(((640.0 - 4.0 - TAB_X0 - text) / gaps).floor()).max(8.0)
    }

    /// The item under the cursor.
    fn current(cx: &Cx) -> usize {
        Self::tab_items(cx)[cx.options.rows[cx.options.tab]] as usize
    }

    /// The cursor is on a gamepad button slot (for the guide).
    pub fn on_pad_slot(cx: &Cx) -> bool {
        !cx.options.tabs.is_empty() && OPTION_ITEMS[Self::current(cx)].kind == Kind::Button
    }

    /// UpdateButtonText (0x41d340), for keys and gamepad buttons.
    fn update_button_text(&mut self, cx: &Cx, item: usize) {
        let code = cx.options.values[item] as i32;
        let name = if OPTION_ITEMS[item].kind == Kind::Button {
            match code.checked_sub(1).and_then(|i| PadButton::BINDABLE.get(i as usize)) {
                Some(b) => b.label(cx.settings.pad_style(cx.input.raw.pad_style)).into(),
                None => "NONE".into(),
            }
        } else {
            let n = Pad::name(code);
            format!("KEY {}", n[0] as char)
        };
        self.items[item] = format!("{}{name}", OPTION_ITEMS[item].text).into_bytes();
    }

    /// The name of value `value` of a formatted item.
    fn value_name(cx: &Cx, item: usize, value: i16) -> String {
        let v = value as usize;
        let s = &cx.settings;
        let on_off = |b| if b { "ON" } else { "OFF" }.to_string();
        match item {
            ITEM_GAME_MODE => MODE_NAMES[v].into(),
            x if x == Item::StartTime as usize && value != 0 => format!("{} SECONDS", START_TIMES[v]),
            x if x == Item::StartTime as usize => "INFINITY".into(),
            x if x == Item::TimerMode as usize => TIMER_MODE_NAMES[v].into(),
            x if x == Item::Ships as usize && value >= 2 => format!("{value} SHIPS"),
            x if x == Item::Ships as usize && value != 0 => "1 SHIP".into(),
            x if x == Item::Ships as usize => "INFINITY".into(),
            x if x == Item::Extend as usize => EXTEND_NAMES[v].into(),
            x if x == Item::DefaultArena as usize => ["CLASSIC", "HD"][v].into(),
            x if x == Item::WindowMode as usize => settings::WINDOW_MODES[v].0.into(),
            x if x == Item::AspectRatio as usize && Self::resolution_locked(cx) => match wf_engine::platform::display_size() {
                Some((w, h)) => settings::aspect_name(settings::aspect(w, h)),
                None => "DISPLAY".into(),
            },
            x if x == Item::AspectRatio as usize => {
                let (w, h) = Self::chosen_size(cx);
                settings::aspect_name(cx.options.aspects.get(v).copied().unwrap_or(settings::aspect(w, h)))
            }
            x if x == Item::Resolution as usize && Self::resolution_locked(cx) => match wf_engine::platform::display_size() {
                Some((w, h)) => format!("{w}X{h}"),
                None => "DISPLAY".into(),
            },
            x if x == Item::Resolution as usize => {
                let (w, h) = cx.options.sizes.get(v).copied().unwrap_or(Self::chosen_size(cx));
                format!("{w}X{h}")
            }
            x if x == Item::RenderScale as usize => format!("{}%", (settings::RENDER_SCALES[v] * 100.0) as i32),
            x if x == Item::FpsCap as usize => match FpsCap::PRESETS.get(v) {
                Some(cap) => cap.label(),
                None if v == FpsCap::PRESETS.len() => match settings::max_refresh_cap() {
                    Some(cap) => format!("MAX ({})", cap.label()),
                    None => "MAX (VSYNC)".into(),
                },
                None => format!("CUSTOM ({})", s.fps_custom),
            },
            x if x == Item::ShowFps as usize && v == 2 => "ON (GPU MS)".into(),
            x if x == Item::ShowFps as usize || x == Item::HideNames as usize || x == Item::MouseSteering as usize => on_off(value != 0),
            x if (Item::MouseLeft as usize..=Item::MouseRight as usize).contains(&x) => settings::MOUSE_ACTIONS[v].0.into(),
            x if x == Item::DeadZone as usize => format!("{}%", (settings::DEAD_ZONES[v] * 100.0).round() as i32),
            x if x == Item::TouchScale as usize => format!("{}X", settings::TOUCH_SCALES[v]),
            x if x == Item::TouchAim as usize => settings::TOUCH_AIMS[v].0.into(),
            x if x == Item::PadNames as usize => settings::PAD_NAMES[v].0.into(),
            _ => String::new(),
        }
    }

    /// UpdateChoiceText (0x41d390).
    fn update_choice_text(&mut self, cx: &mut Cx, item: usize) {
        let value = cx.options.values[item];
        let name = Self::value_name(cx, item, value);
        let (left, right) = if cx.options.enabled[item] {
            (if value != 0 { "< " } else { "  " }, if value < cx.options.count(item) - 1 { " >" } else { "" })
        } else {
            ("  ", "")
        };
        self.items[item] = format!("{}{left}{name}{right}", OPTION_ITEMS[item].text).into_bytes();
        cx.options.widths[item] = (name.len() * 8) as i16;
    }

    /// A borderless window takes the display's resolution, so ASPECT
    /// RATIO and RESOLUTION are locked to it and show it.
    fn resolution_locked(cx: &Cx) -> bool {
        cx.settings.display.mode == wf_engine::config::WindowMode::Borderless
    }

    /// The size ASPECT RATIO and RESOLUTION choose: the window's, or in
    /// fullscreen the display mode's (the desktop's until one is picked).
    fn chosen_size(cx: &Cx) -> (u32, u32) {
        let d = &cx.settings.display;
        match d.mode {
            wf_engine::config::WindowMode::Fullscreen => {
                d.fullscreen_size.or_else(wf_engine::platform::display_size).unwrap_or((d.width, d.height))
            }
            _ => (d.width, d.height),
        }
    }

    fn set_chosen_size(cx: &mut Cx, size: (u32, u32)) {
        let d = &mut cx.settings.display;
        match d.mode {
            wf_engine::config::WindowMode::Fullscreen => d.fullscreen_size = Some(size),
            _ => (d.width, d.height) = size,
        }
    }

    /// ASPECT RATIO and RESOLUTION follow the window mode: the window's
    /// size or the fullscreen mode, locked to the display's when
    /// borderless.
    fn sync_resolution(&mut self, cx: &mut Cx) {
        let size = Self::chosen_size(cx);
        cx.options.list_sizes(size);
        for item in [Item::AspectRatio as usize, Item::Resolution as usize] {
            cx.options.enabled[item] = !Self::resolution_locked(cx);
            self.update_choice_text(cx, item);
        }
    }

    /// The port's items' values, from the settings.
    fn load_port_values(cx: &mut Cx) {
        let s = &cx.settings;
        let v = [
            (Item::DefaultArena, (cx.arena_choice == super::arena::Mode::Hd) as usize),
            (Item::WindowMode, s.window_mode_index()),
            (Item::RenderScale, settings::nearest(&settings::RENDER_SCALES, s.display.render_scale)),
            (Item::FpsCap, s.fps_index()),
            (Item::ShowFps, if s.perf_overlay { 2 } else { s.display.show_fps as usize }),
            (Item::HideNames, s.hide_names as usize),
            (Item::MouseLeft, s.mouse[0] as usize),
            (Item::MouseMiddle, s.mouse[1] as usize),
            (Item::MouseRight, s.mouse[2] as usize),
            (Item::MouseSteering, s.mouse_steering as usize),
            (Item::DeadZone, settings::nearest(&settings::DEAD_ZONES, s.dead_zone)),
            (Item::TouchScale, settings::nearest(&settings::TOUCH_SCALES, s.touch_scale)),
            (Item::TouchAim, s.touch_aim as usize),
            (Item::PadNames, s.pad_names as usize),
        ];
        for (item, value) in v {
            cx.options.values[item as usize] = value as i16;
        }
        let size = Self::chosen_size(cx);
        cx.options.list_sizes(size);
    }

    /// A port item changed: the settings follow at once (the display and
    /// the bindings apply it live).
    fn store_port_value(cx: &mut Cx, item: usize, value: i16) {
        let v = value as usize;
        let s = &mut cx.settings;
        match item {
            x if x == Item::DefaultArena as usize => {
                cx.arena_choice = if v == 1 { super::arena::Mode::Hd } else { super::arena::Mode::Classic };
                return;
            }
            x if x == Item::WindowMode as usize => {
                s.display.mode = settings::WINDOW_MODES[v].1;
                // Into fullscreen at the display's current mode, whatever
                // was picked before (RESOLUTION can change it after).
                if s.display.mode == wf_engine::config::WindowMode::Fullscreen {
                    s.display.fullscreen_size = None;
                }
            }
            // Another ratio: its size nearest the window's in area.
            x if x == Item::AspectRatio as usize => {
                let Some(&ratio) = cx.options.aspects.get(v) else { return };
                let area = |(w, h): (u32, u32)| w as f64 * h as f64;
                let now = area(Self::chosen_size(cx));
                let all = settings::resolutions();
                let sizes = settings::sizes_of(&all, ratio);
                let Some(&size) = sizes.iter().min_by(|&&a, &&b| (area(a) / now).ln().abs().total_cmp(&(area(b) / now).ln().abs())) else {
                    return;
                };
                Self::set_chosen_size(cx, size);
                cx.options.list_sizes(size);
                cx.settings.display_dirty = true;
                return;
            }
            x if x == Item::Resolution as usize => {
                let Some(&size) = cx.options.sizes.get(v) else { return };
                Self::set_chosen_size(cx, size);
                cx.settings.display_dirty = true;
                return;
            }
            x if x == Item::RenderScale as usize => s.display.render_scale = settings::RENDER_SCALES[v],
            x if x == Item::FpsCap as usize => s.set_fps_index(v),
            x if x == Item::ShowFps as usize => {
                s.display.show_fps = v != 0;
                s.perf_overlay = v == 2;
            }
            x if x == Item::HideNames as usize => {
                s.hide_names = v != 0;
                return;
            }
            x if (Item::MouseLeft as usize..=Item::MouseRight as usize).contains(&x) => {
                s.mouse[x - Item::MouseLeft as usize] = v as u8;
                s.input_dirty = true;
                return;
            }
            x if x == Item::MouseSteering as usize => {
                s.mouse_steering = v != 0;
                s.input_dirty = true;
                return;
            }
            x if x == Item::DeadZone as usize => {
                s.dead_zone = settings::DEAD_ZONES[v];
                s.input_dirty = true;
                return;
            }
            x if x == Item::TouchScale as usize => {
                s.touch_scale = settings::TOUCH_SCALES[v];
                s.input_dirty = true;
                return;
            }
            x if x == Item::TouchAim as usize => {
                s.touch_aim = v as u8;
                return;
            }
            x if x == Item::PadNames as usize => {
                s.pad_names = v as u8;
                return;
            }
            _ => return,
        }
        s.display_dirty = true;
    }

    /// Binds a pad slot, and keeps wf.ini's one button per trigger (the
    /// original's) at the action's first button where it has one.
    fn set_pad_slot(cx: &mut Cx, k: usize, slot: usize, button: Option<PadButton>) {
        cx.settings.set_pad_slot(k, slot, button);
        let t = settings::PAD_ACTIONS[k].0 as usize;
        if let Some(first) = cx.settings.pad[k].iter().flatten().next()
            && let Some(i) = PadButton::TRIGGERS.iter().position(|b| b == first)
        {
            cx.input.pads[0].button_map[t] = i as u8;
        }
    }

    fn play(cx: &mut Cx, id: usize) {
        cx.play_sound(id);
    }

    /// Leaves the screen (EXIT, or cancel from any tab).
    fn exit(&mut self, cx: &mut Cx) {
        cx.sound.fade(super::assets::CHANNEL_BGM, 1.0, 0.0, 1.0, true);
        cx.fade.fade_out(60);
        self.flags |= FLAG_EXIT;
        Self::play(cx, sounds::SWITCH);
    }

    fn switch_tab(&mut self, cx: &mut Cx, tab: usize, dir: f32) {
        if tab == cx.options.tab {
            return;
        }
        cx.options.tab = tab;
        self.slide = SLIDE_FRAMES;
        self.slide_dir = dir;
        self.blink = 255;
        Self::play(cx, sounds::FIRE_ME);
    }

    /// The tab under `p`, if it is on the strip.
    fn tab_at(cx: &Cx, p: [f32; 2]) -> Option<usize> {
        if !(TAB_Y - 6.0..TAB_Y + 14.0).contains(&p[1]) {
            return None;
        }
        (0..cx.options.tabs.len()).rev().find(|&i| p[0] >= Self::tab_x(cx, i) - 4.0)
    }

    /// The row under `p`, if it is on an item.
    fn row_at(cx: &Cx, p: [f32; 2]) -> Option<usize> {
        if p[0] < 312.0 || p[1] < ROW_Y - 4.0 {
            return None;
        }
        let row = ((p[1] - ROW_Y + 4.0) / ROW_H) as usize;
        (row < Self::tab_items(cx).len()).then_some(row)
    }

    /// Where the tabs and items are drawn from: kept to the window's right
    /// edge, as they keep to the 640x480 screen's in the original.
    fn block_origin(cx: &Cx) -> [f32; 2] {
        let [vx, _, vw, _] = cx.screen_view();
        [vx + vw - 640.0, 0.0]
    }

    /// The tab-switch prompt for the device used last.
    fn hint(cx: &Cx) -> &'static str {
        match cx.ui.shown.unwrap_or(cx.ui.device) {
            Device::Keyboard => "Q / E : SWITCH TAB",
            Device::Gamepad => "LB / RB : SWITCH TAB",
            // The tabs are plain to click.
            Device::Mouse => "",
            Device::Touch => "TAP OR SWIPE : SWITCH TAB",
        }
    }

    /// The pointer and the tab keys (display only: the options are not
    /// recorded). Returns the menu bits a click makes.
    fn pointer(&mut self, cx: &mut Cx, remap: Option<Kind>) -> u16 {
        let mut ui = cx.ui.clone();
        // Into the block's own units (it is drawn moved by `block_origin`).
        let o = Self::block_origin(cx);
        let local = |p: [f32; 2]| [p[0] - o[0], p[1] - o[1]];
        ui.cursor = ui.cursor.map(local);
        ui.click = ui.click.map(local);
        ui.right_click = ui.right_click.map(local);
        let tab = cx.options.tab;
        let n = cx.options.tabs.len();
        // A key or button being remapped wins over the tab keys it shares.
        let blocked = match remap {
            Some(Kind::Key) => ui.tab_by == TabBy::Letter,
            Some(Kind::Button) => ui.tab_by == TabBy::Shoulder,
            _ => false,
        };
        if ui.tab != 0 && !blocked {
            let to = (tab as i32 + ui.tab as i32).rem_euclid(n as i32) as usize;
            self.switch_tab(cx, to, ui.tab as f32);
            return 0;
        }
        if let Some(p) = ui.cursor
            && ui.wheel != 0.0
            && Self::tab_at(cx, [p[0].max(TAB_X0), p[1]]).is_some()
        {
            let dir = if ui.wheel < 0.0 { 1 } else { -1 };
            let to = (tab as i32 + dir).rem_euclid(n as i32) as usize;
            self.switch_tab(cx, to, dir as f32);
            return 0;
        }
        // The back button leaves (a remap row takes it as nothing); START
        // always does.
        if (ui.back && remap.is_none()) || ui.start {
            self.exit(cx);
            return 0;
        }
        if let Some(p) = ui.cursor
            && ui.moved
            && ui.device == Device::Mouse
            && let Some(row) = Self::row_at(cx, p)
            && cx.options.enabled[Self::tab_items(cx)[row] as usize]
            && row != cx.options.rows[tab]
        {
            cx.options.rows[tab] = row;
            self.blink = 255;
            Self::play(cx, sounds::FIRE_ME);
        }
        // The wheel over an adjustable row steps its value (up: the next
        // one); elsewhere it moves the cursor.
        if ui.nav != 0 {
            let row = ui.cursor.and_then(|p| Self::row_at(cx, p)).filter(|&r| Self::adjustable(cx, Self::tab_items(cx)[r] as usize));
            if let Some(row) = row {
                cx.options.rows[tab] = row;
                return if ui.nav < 0 { input::RIGHT } else { input::LEFT };
            }
            return if ui.nav < 0 { input::UP } else { input::DOWN };
        }
        // The right button steps a value back.
        if let Some(p) = ui.right_click {
            if let Some(row) = Self::row_at(cx, p).filter(|&r| Self::adjustable(cx, Self::tab_items(cx)[r] as usize)) {
                cx.options.rows[tab] = row;
                self.blink = 255;
                return input::LEFT;
            }
            return 0;
        }
        let Some(p) = ui.click else { return 0 };
        if let Some(t) = Self::tab_at(cx, p) {
            self.switch_tab(cx, t, if t > tab { 1.0 } else { -1.0 });
            return 0;
        }
        let Some(row) = Self::row_at(cx, p) else { return 0 };
        let item = Self::tab_items(cx)[row] as usize;
        if !cx.options.enabled[item] {
            return 0;
        }
        cx.options.rows[tab] = row;
        self.blink = 255;
        match OPTION_ITEMS[item].kind {
            Kind::Choice | Kind::Formatted => Self::value_step(item, cx.options.values[item], cx.options.widths[item], &self.items[item], p[0]),
            Kind::Action => input::SHOT,
            _ => 0,
        }
    }

    /// An enabled item with values to step through.
    fn adjustable(cx: &Cx, item: usize) -> bool {
        matches!(OPTION_ITEMS[item].kind, Kind::Choice | Kind::Formatted) && cx.options.enabled[item]
    }

    /// Where a choice's pointer arrows go: left of its values, and right
    /// of its text.
    fn choice_arrows(text: &[u8]) -> [f32; 2] {
        let len = text.iter().rposition(|&c| c != b' ').map_or(0, |i| i + 1);
        [456.0, 320.0 + len as f32 * 8.0 + 8.0]
    }

    /// The step a click at `x` on a value row makes: on the left arrow or
    /// the left of the values, back; on the right, forward.
    fn value_step(item: usize, value: i16, width: i16, text: &[u8], x: f32) -> u16 {
        let o = &OPTION_ITEMS[item];
        if o.kind == Kind::Choice {
            let [l, r] = Self::choice_arrows(text);
            if (l - 2.0..l + 10.0).contains(&x) {
                return input::LEFT;
            }
            if x >= r - 2.0 {
                return input::RIGHT;
            }
        }
        // Left of the value shown steps down, right of it up (a choice
        // shows all its values, the chosen one boxed).
        let at = if o.kind == Kind::Choice { value as f32 * o.param as f32 } else { 0.0 };
        let mid = 464.0 + o.column as f32 + at + width as f32 * 0.5;
        if x < mid { input::LEFT } else { input::RIGHT }
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
        Self::set_tabs(cx);
        Self::load_port_values(cx);
        for i in 0..ITEMS {
            match OPTION_ITEMS[i].kind {
                Kind::Formatted => self.update_choice_text(cx, i),
                Kind::Button | Kind::Key => {
                    cx.options.values[i] = trigger(cx, i);
                    self.update_button_text(cx, i);
                }
                _ => self.items[i] = OPTION_ITEMS[i].text.as_bytes().to_vec(),
            }
        }
        self.sync_resolution(cx);
        self.flags = 0;
        self.blink = 0;
        self.slide = 0;
        cx.fade.set_color(0, 0, 0, 255);
        cx.fade.fade_to(60, 0, 0, 0, 0);
        true
    }

    /// Finalize (0x41ded0).
    fn finalize(&mut self, cx: &mut Cx) {
        apply(cx);
        save_settings(cx);
        let Cx { settings, port, .. } = cx;
        settings.store(port);
        super::arena::save_choice(cx);
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
        if self.slide > 0 {
            self.slide -= 1;
        }
        // Another kind of gamepad: its buttons' names.
        let style = cx.settings.pad_style(cx.input.raw.pad_style);
        if style != self.pad_style {
            self.pad_style = style;
            for i in (Item::PadShot1 as usize)..=(Item::PadWeapon3 as usize) {
                self.update_button_text(cx, i);
            }
        }
        if cx.fade.fade_frames() != 0 {
            return;
        }
        if self.flags & FLAG_EXIT != 0 {
            cx.app.finish(Some(Box::new(super::fn_title::FnTitle::new())));
            return;
        }
        let c = Self::current(cx);
        let kind = OPTION_ITEMS[c].kind;
        let remap = matches!(kind, Kind::Key | Kind::Button).then_some(kind);
        let mut pressed = cx.input.pad(0).pressed;
        // The pointer's buttons act through clicks on the screen.
        if matches!(cx.ui.device, Device::Mouse | Device::Touch) {
            pressed &= !(input::SHOT | input::SLOW | input::WEAPON_SELECT);
        }
        let tab_before = cx.options.tab;
        pressed |= self.pointer(cx, remap);
        if cx.options.tab != tab_before {
            return;
        }
        let tab = cx.options.tab;
        let c = Self::current(cx);
        if pressed & (input::UP | input::DOWN) != 0 {
            let rows = Self::tab_items(cx);
            let mut row = cx.options.rows[tab];
            loop {
                if pressed & input::UP != 0 {
                    row = if row == 0 { rows.len() - 1 } else { row - 1 };
                } else {
                    row = if row + 1 >= rows.len() { 0 } else { row + 1 };
                }
                if cx.options.enabled[rows[row] as usize] {
                    break;
                }
            }
            cx.options.rows[tab] = row;
            Self::play(cx, sounds::FIRE_ME);
            self.blink = 255;
            self.flags |= FLAG_HELD;
            return;
        }
        // Cancel leaves from any tab (a remap row takes the press instead).
        if pressed & input::SLOW != 0 && remap.is_none() {
            self.exit(cx);
            return;
        }
        match kind {
            Kind::Choice | Kind::Formatted => {
                let old = cx.options.values[c];
                let count = cx.options.count(c);
                let v = &mut cx.options.values[c];
                if pressed & input::LEFT != 0 {
                    *v = (*v - 1).clamp(0, count - 1);
                } else if pressed & input::RIGHT != 0 {
                    *v += 1;
                    if *v >= count {
                        *v = count - 1;
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
                    x if x == Item::Ships as usize
                        && value == 0 && cx.options.values[Item::StartTime as usize] == 0 => {
                            cx.options.values[Item::StartTime as usize] = 1;
                        }
                    x if x == Item::WindowMode as usize || x == Item::AspectRatio as usize => {
                        Self::store_port_value(cx, x, value);
                        self.sync_resolution(cx);
                    }
                    x if x >= Item::DefaultArena as usize => Self::store_port_value(cx, x, value),
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
                if pressed & (input::SHOT | input::SLOW | input::WEAPON_SELECT) != 0 && c == Item::Exit as usize {
                    self.exit(cx);
                }
            }
            Kind::Button | Kind::Key => {
                // Only the item's own device remaps it. A pad slot takes
                // any bindable button; BACK (SELECT) clears it.
                let raw = &cx.input.raw;
                let code = if kind == Kind::Key {
                    input::TRIGGER_KEYS.iter().position(|&k| raw.key(k)).map(|i| i as i32)
                } else if raw.gamepads.iter().any(|g| g.held(PadButton::Back)) {
                    Some(0)
                } else {
                    raw.gamepads
                        .iter()
                        .find_map(|g| PadButton::BINDABLE.iter().position(|&b| g.held(b)))
                        .map(|b| b as i32 + 1)
                };
                let Some(code) = code else {
                    self.flags &= !FLAG_HELD;
                    return;
                };
                if self.flags & FLAG_HELD != 0 {
                    return;
                }
                self.flags |= FLAG_HELD;
                Self::play(cx, sounds::SWITCH);
                if code as i16 != cx.options.values[c] {
                    if kind == Kind::Key {
                        cx.input.pads[0].set_trigger(OPTION_ITEMS[c].param as usize, code);
                        cx.options.values[c] = code as i16;
                        self.update_button_text(cx, c);
                    } else {
                        let t = OPTION_ITEMS[c].param as usize;
                        let button = (code as usize).checked_sub(1).map(|i| PadButton::BINDABLE[i]);
                        Self::set_pad_slot(cx, t / settings::PAD_SLOTS, t % settings::PAD_SLOTS, button);
                        // A button moved from another slot: every slot's text.
                        for i in (Item::PadShot1 as usize)..=(Item::PadWeapon3 as usize) {
                            cx.options.values[i] = trigger(cx, i);
                            self.update_button_text(cx, i);
                        }
                    }
                }
            }
        }
    }

    /// Draw (0x41d880), with the tab strip over the items.
    fn draw(&mut self, cx: &mut Cx) {
        cx.guide = Some(super::guide::Guide::Options);
        let mut t = Transform::IDENTITY;
        t.scale(65.0, 65.0);
        let t = super::background::rotate(&t, cx.app.frame_count.wrapping_mul(3) as i32);
        let mut t = t;
        t.translate(300.0, 320.0);
        cx.draw.sprite(cx.art.shapes.hexagon, &t, Argb(0x6400_ff80), Blend::Normal);

        let id = Transform::IDENTITY;
        cx.draw.set_origin(Self::block_origin(cx));
        // The tab strip: the current tab boxed and underlined.
        let tab = cx.options.tab;
        for (i, (name, _)) in cx.options.tabs.clone().iter().enumerate() {
            let x = Self::tab_x(cx, i);
            let w = name.len() as f32 * 8.0;
            let color = if i == tab { Argb(0xe0ff_ffff) } else { Argb(0x6080_ffc0) };
            if i == tab {
                let quad = [[x - 4.0, TAB_Y - 3.0], [x + w + 4.0, TAB_Y - 3.0], [x - 4.0, TAB_Y + 11.0], [x + w + 4.0, TAB_Y + 11.0]];
                cx.draw.shape(Primitive::TriangleStrip, &id, &quad, Argb(0x5000_ff80), Blend::Normal);
                let line = [[x - 4.0, TAB_Y + 13.0], [x + w + 4.0, TAB_Y + 13.0]];
                cx.draw.shape(Primitive::LineList, &id, &line, Argb(0xc0ff_dd00), Blend::Normal);
            }
            cx.fonts.small_text.print(&mut cx.draw, x, TAB_Y, name.as_bytes(), None, color, -1, Blend::Normal);
        }
        let hint = Self::hint(cx);
        if !hint.is_empty() {
            cx.fonts.small_text.print(&mut cx.draw, 320.0, TAB_Y + 20.0, hint.as_bytes(), None, Argb(0x5080_ffc0), -1, Blend::Normal);
        }

        // The items, sliding in on a tab switch.
        let k = self.slide as f32 / SLIDE_FRAMES as f32;
        let dx = (self.slide_dir * 48.0 * k * k).trunc();
        let fade = 1.0 - k;
        let items = Self::tab_items(cx);
        for (row, &item) in items.iter().enumerate() {
            let i = item as usize;
            let y = ROW_Y + row as f32 * ROW_H;
            let o = &OPTION_ITEMS[i];
            if o.count != 0 && cx.options.enabled[i] {
                let x0 = (cx.options.values[i].clamp(0, o.count - 1) as i32 * o.param as i32 + o.column as i32 + 464) as f32 + dx;
                let x1 = cx.options.widths[i] as f32 + x0;
                let quad = [[x0, y], [x1, y], [x0, y + 8.0], [x1, y + 8.0]];
                cx.draw.shape(Primitive::TriangleStrip, &id, &quad, Argb(((0x64 as f32 * fade) as u32) << 24 | 0xff80), Blend::Normal);
            }
        }
        let row = cx.options.rows[tab];
        let c = items[row] as usize;
        let len = self.items[c].len() as f32;
        let cy = row as f32 * ROW_H + ROW_Y + 8.0;
        let line = [[320.0 + dx, cy], [len * 8.0 + 320.0 + dx, cy]];
        cx.draw.shape(Primitive::LineList, &id, &line, Argb(((self.blink as u32) << 24) | 0xffdd00), Blend::Normal);
        let text = Argb((((0x80 as f32) * fade) as u32) << 24 | 0xff_ffff);
        for (row, &item) in items.iter().enumerate() {
            let y = ROW_Y + row as f32 * ROW_H;
            cx.fonts.small_text.print(&mut cx.draw, 320.0 + dx, y.trunc(), &self.items[item as usize], None, text, -1, Blend::Normal);
            // The pointer's step arrows beside a choice's values (the
            // formatted items carry theirs in the text).
            let i = item as usize;
            if cx.ui.shows_pointer() && OPTION_ITEMS[i].kind == Kind::Choice && cx.options.enabled[i] {
                let [l, r] = Self::choice_arrows(&self.items[i]);
                let v = cx.options.values[i];
                let arrow = Argb((((0xc0 as f32) * fade) as u32) << 24 | 0xffdd00);
                if v > 0 {
                    cx.fonts.small_text.print(&mut cx.draw, l + dx, y.trunc(), b"<", None, arrow, -1, Blend::Normal);
                }
                if v < OPTION_ITEMS[i].count - 1 {
                    cx.fonts.small_text.print(&mut cx.draw, r + dx, y.trunc(), b">", None, arrow, -1, Blend::Normal);
                }
            }
        }
        // The band, at the window's bottom and repeated across its width
        // (the text repeats every 64 units, so the copies join seamlessly).
        cx.draw.set_origin([0.0, 0.0]);
        let [vx, vy, vw, vh] = cx.screen_view();
        let scroll = ((cx.app.frame_count & 0x3f) * 2) as f32;
        let mut x = -scroll + ((vx + scroll) / 768.0).floor() * 768.0;
        while x < vx + vw {
            let mut t = Transform::IDENTITY;
            t.m[2][0] = x;
            t.m[2][1] = vy + vh - 30.0;
            texels::draw(cx, &texels::font2_strip(1), &t, &TITLE_BAR, &TITLE_BAR_TEXELS, 0xc800_ff80);
            x += 768.0;
        }
    }

    fn is_menu(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_hits_tabs_rows_and_value_arrows() {
        let cx = super::super::tests::cx();
        // The desktop strip: GAME DISPLAY AUDIO CONTROLS PAD RECORDS,
        // closer than TAB_GAP to fit the screen.
        let xs: Vec<f32> = (0..6).map(|i| FnOption::tab_x(&cx, i)).collect();
        assert_eq!(xs, [320.0, 360.0, 424.0, 472.0, 544.0, 576.0]);
        assert!(xs[5] + 7.0 * 8.0 <= 640.0);
        assert_eq!(FnOption::tab_at(&cx, [328.0, TAB_Y + 4.0]), Some(0));
        assert_eq!(FnOption::tab_at(&cx, [584.0, TAB_Y + 4.0]), Some(5));
        assert_eq!(FnOption::tab_at(&cx, [448.0, TAB_Y + 40.0]), None);
        assert_eq!(FnOption::row_at(&cx, [330.0, ROW_Y + 2.0]), Some(0));
        assert_eq!(FnOption::row_at(&cx, [330.0, ROW_Y + ROW_H * 2.0 + 2.0]), Some(2));
        assert_eq!(FnOption::row_at(&cx, [100.0, ROW_Y + 2.0]), None);
        assert_eq!(FnOption::row_at(&cx, [330.0, ROW_Y + ROW_H * 40.0]), None);
        // A choice: its arrows, and either side of the value shown.
        let boss = Item::BossName as usize;
        let text = OPTION_ITEMS[boss].text.as_bytes();
        let [l, r] = FnOption::choice_arrows(text);
        assert_eq!(l, 456.0);
        assert_eq!(FnOption::value_step(boss, 1, 56, text, l + 4.0), input::LEFT);
        assert_eq!(FnOption::value_step(boss, 0, 64, text, r + 4.0), input::RIGHT);
        // ENGLISH (value 1) is boxed at 536..592: JAPANESE, left of it, steps back.
        assert_eq!(FnOption::value_step(boss, 1, 56, text, 480.0), input::LEFT);
        assert_eq!(FnOption::value_step(boss, 0, 64, text, 560.0), input::RIGHT);
        // A formatted item: "< NAME >", the left half back, the right on.
        let mode = Item::GameMode as usize;
        assert_eq!(FnOption::value_step(mode, 0, 48, b"", 466.0), input::LEFT);
        assert_eq!(FnOption::value_step(mode, 0, 48, b"", 540.0), input::RIGHT);
        assert!(FnOption::adjustable(&cx, mode));
        assert!(!FnOption::adjustable(&cx, Item::Exit as usize));
        assert!(!FnOption::adjustable(&cx, Item::ShotKey as usize));
    }

    #[test]
    fn aspect_ratio_lists_the_sizes_of_its_ratio() {
        let mut cx = super::super::tests::cx();
        // The fallback list (no display modes in tests): 16:9 1280x720 is
        // among the 16:9 sizes, smallest first.
        cx.options.list_sizes((1280, 720));
        assert_eq!(cx.options.sizes[0], (1280, 720));
        assert!(cx.options.sizes.iter().all(|&(w, h)| settings::aspect(w, h) == (16, 9)));
        assert_eq!(cx.options.aspects[cx.options.values[Item::AspectRatio as usize] as usize], (16, 9));
        // Another ratio takes its size nearest in area.
        let four_three = cx.options.aspects.iter().position(|&r| r == (4, 3)).unwrap();
        FnOption::store_port_value(&mut cx, Item::AspectRatio as usize, four_three as i16);
        assert_eq!((cx.settings.display.width, cx.settings.display.height), (1024, 768));
        // A size of no listed ratio is listed with its own.
        cx.options.list_sizes((1280, 800));
        assert!(cx.options.aspects.contains(&(16, 10)));
        assert_eq!(cx.options.sizes, [(1280, 800)]);
    }

    #[test]
    fn pad_slots_take_a_button_from_elsewhere() {
        use wf_engine::platform::PadButton as P;
        let mut cx = super::super::tests::cx();
        assert_eq!(cx.settings.pad[0], [Some(P::South), Some(P::RightShoulder), Some(P::RightTrigger)]);
        FnOption::set_pad_slot(&mut cx, 2, 1, Some(P::RightShoulder));
        assert_eq!(cx.settings.pad[0], [Some(P::South), None, Some(P::RightTrigger)]);
        assert_eq!(cx.settings.pad[2], [Some(P::North), Some(P::RightShoulder), Some(P::LeftTrigger)]);
        FnOption::set_pad_slot(&mut cx, 0, 0, Some(P::West));
        // wf.ini's trigger map follows the first button.
        assert_eq!(cx.input.pad(0).button_map[0], 2);
    }

    #[test]
    fn mobile_omits_the_window_rows() {
        let display = |caps: &Capabilities| build_tabs(caps).into_iter().find(|(n, _)| *n == "DISPLAY").unwrap().1;
        assert_eq!(display(&Capabilities::DESKTOP), TABS[1].1);
        assert_eq!(display(&Capabilities::MOBILE), [Item::RenderScale, Item::FpsCap, Item::ShowFps, Item::HideNames, Item::BossName, Item::Exit]);
        // No tab is left empty, so all six stay; the other tabs are as on a desktop.
        let tabs = build_tabs(&Capabilities::MOBILE);
        assert_eq!(tabs.len(), 6);
        assert!(tabs.iter().zip(TABS).all(|((a, items), (b, all))| a == &b && (a == &"DISPLAY" || items == all)));

        // The cursor on RESOLUTION moves to EXIT when the row goes; on
        // RENDER SCALE it stays there.
        let mut cx = super::super::tests::cx();
        cx.options.tab = 1;
        cx.options.rows[1] = 1;
        cx.options.rows[2] = 2;
        cx.settings.set_caps(Capabilities::MOBILE);
        FnOption::set_tabs(&mut cx);
        assert_eq!(FnOption::current(&cx), Item::Exit as usize);
        assert_eq!(cx.options.rows[2], 2);
        cx.options.rows[1] = 0;
        cx.settings.set_caps(Capabilities::DESKTOP);
        FnOption::set_tabs(&mut cx);
        assert_eq!(FnOption::current(&cx), Item::RenderScale as usize);
        assert_eq!(cx.options.rows[1], 3);
    }

    #[test]
    fn borderless_locks_the_resolution_to_the_display() {
        use wf_engine::config::WindowMode;
        wf_engine::platform::set_display_size(2560, 1440);
        let mut cx = super::super::tests::cx();
        let mut f = FnOption::new();
        let item = Item::Resolution as usize;
        let text = |f: &FnOption| String::from_utf8_lossy(&f.items[item]).into_owned();
        (cx.settings.display.width, cx.settings.display.height) = (1280, 720);
        cx.settings.display.mode = WindowMode::Windowed;
        FnOption::load_port_values(&mut cx);
        f.sync_resolution(&mut cx);
        assert!(cx.options.enabled[item]);
        assert!(text(&f).contains("1280X720"), "{}", text(&f));
        cx.settings.display.mode = WindowMode::Borderless;
        f.sync_resolution(&mut cx);
        assert!(!cx.options.enabled[item]);
        assert!(text(&f).ends_with(" 2560X1440"), "{}", text(&f));
        // The windowed size is kept for going back to a window.
        assert_eq!((cx.settings.display.width, cx.settings.display.height), (1280, 720));
        // Fullscreen picks its own mode: the desktop's until one is chosen,
        // and again whenever WINDOW MODE switches to it.
        cx.settings.display.fullscreen_size = Some((1280, 720));
        FnOption::store_port_value(&mut cx, Item::WindowMode as usize, 2);
        assert_eq!(cx.settings.display.mode, WindowMode::Fullscreen);
        assert_eq!(cx.settings.display.fullscreen_size, None);
        f.sync_resolution(&mut cx);
        assert!(cx.options.enabled[item]);
        assert!(text(&f).contains("2560X1440"), "{}", text(&f));
        let aspect = String::from_utf8_lossy(&f.items[Item::AspectRatio as usize]).into_owned();
        assert!(aspect.contains("16:9"), "{aspect}");
        FnOption::store_port_value(&mut cx, item, 0);
        assert!(cx.settings.display.fullscreen_size.is_some());
        assert_eq!((cx.settings.display.width, cx.settings.display.height), (1280, 720));
        cx.settings.display.mode = WindowMode::Windowed;
        f.sync_resolution(&mut cx);
        assert!(cx.options.enabled[item]);
        assert!(text(&f).contains("1280X720"));
    }

    #[test]
    fn hidden_display_settings_ignore_the_ini() {
        use wf_engine::config::{DisplayConfig, WindowMode};
        let mut d = DisplayConfig { width: 2560, height: 1440, mode: WindowMode::Borderless, render_scale: 1.5, ..Default::default() };
        let kept = d;
        Capabilities::DESKTOP.mask(&mut d);
        assert_eq!(d, kept);
        Capabilities::MOBILE.mask(&mut d);
        let default = DisplayConfig::default();
        assert_eq!((d.width, d.height, d.mode), (default.width, default.height, default.mode));
        assert_eq!(d.render_scale, 1.5);
    }
}
