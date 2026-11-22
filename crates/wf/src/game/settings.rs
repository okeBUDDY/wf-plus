//! The port's own settings (wf-plus.ini), which the options screen edits
//! beside the original's [Option] ones: the display, the mouse and touch
//! bindings and tuning. Nothing here is game state or goes into a .rec.

use wf_engine::config::{Config, DisplayConfig, WindowMode};
use wf_engine::input::{Action, Bindings, Source};
use wf_engine::platform::{Capabilities, Device};
use wf_engine::time::FpsCap;

/// What a mouse button does (MOUSE LEFT/MIDDLE/RIGHT).
pub const MOUSE_ACTIONS: [(&str, Option<Action>); 5] = [
    ("SHOT", Some(Action::SHOT)),
    ("SLOW", Some(Action::SLOW)),
    ("WEAPON", Some(Action::WEAPON)),
    ("PAUSE", Some(Action::PAUSE)),
    ("NONE", None),
];
const MOUSE_KEYS: [&str; 3] = ["input.mouse_left", "input.mouse_middle", "input.mouse_right"];
/// Left shoots, middle selects the weapon, right moves slowly.
const MOUSE_DEFAULTS: [u8; 3] = [0, 2, 1];

/// The window sizes the options offer (16:9 first).
pub const RESOLUTIONS: [(u32, u32); 9] =
    [(1280, 720), (1600, 900), (1920, 1080), (2560, 1440), (3840, 2160), (640, 480), (1024, 768), (1280, 960), (2560, 1080)];
pub const RENDER_SCALES: [f32; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];
pub const DEAD_ZONES: [f32; 10] = [0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45, 0.5];
pub const TOUCH_SCALES: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0];

/// How a touch player aims the laser's cone (which moves only while the
/// shot is up): (options name, wf-plus.ini `input.touch_aim`).
/// 0: a FIRE button on screen turns the shot on and off. 1 (the default):
/// a second finger held holds the shot (the steering finger's
/// double-tap-and-hold is then the slow move). 2: a slow drag holds the
/// shot.
pub const TOUCH_AIMS: [(&str, &str); 3] = [("FIRE BUTTON", "button"), ("2ND FINGER", "finger"), ("SLOW DRAG", "drag")];

#[derive(Clone, Debug)]
pub struct Settings {
    pub display: DisplayConfig,
    /// The FPS cap's CUSTOM rate.
    pub fps_custom: f64,
    /// The display changed (the engine applies it).
    pub display_dirty: bool,
    /// Index into `MOUSE_ACTIONS` of the left, middle and right buttons.
    pub mouse: [u8; 3],
    pub mouse_steering: bool,
    pub dead_zone: f32,
    pub touch_scale: f32,
    /// Index into `TOUCH_AIMS`.
    pub touch_aim: u8,
    /// The input tuning changed (the bindings follow).
    pub input_dirty: bool,
    /// The display rate measured last, for SHOW FPS.
    pub fps: f32,
    /// SHOW FPS with the frame's GPU time (`ui.perf_overlay`).
    pub perf_overlay: bool,
    /// The GPU time of a frame (ms; None without timestamp queries) and
    /// the present-to-present time, for the perf overlay.
    pub gpu_ms: Option<f32>,
    pub frame_ms: f32,
    /// Draw the heavy sprites from the sprite cache (`gfx.sprite_cache`).
    pub sprite_cache: bool,
    /// Draw the line shapes and small fills analytically, not with Slug (`gfx.analytic_shapes`).
    pub analytic_shapes: bool,
    /// When the input guide shows (`ui.input_guide`).
    pub input_guide: super::guide::Policy,
    /// The device the guide names, when forced (`--guide`, screenshots).
    pub guide_device: Option<Device>,
    /// What the platform lets the options change (`--caps` simulates
    /// another platform's).
    pub caps: Capabilities,
    /// Streamer mode (`ui.hide_names`): the players' initials are shown
    /// as "***" (display only; the scores keep them).
    pub hide_names: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            display: DisplayConfig::default(),
            fps_custom: 90.0,
            display_dirty: false,
            mouse: MOUSE_DEFAULTS,
            mouse_steering: true,
            dead_zone: 0.35,
            touch_scale: 1.0,
            touch_aim: 1,
            input_dirty: true,
            fps: 0.0,
            perf_overlay: false,
            gpu_ms: None,
            frame_ms: 0.0,
            sprite_cache: true,
            analytic_shapes: true,
            input_guide: Default::default(),
            guide_device: None,
            caps: wf_engine::platform::capabilities(),
            hide_names: false,
        }
    }
}

/// The index of the entry of `list` nearest `v`.
pub fn nearest(list: &[f32], v: f32) -> usize {
    let mut best = 0;
    for (i, &x) in list.iter().enumerate() {
        if (x - v).abs() < (list[best] - v).abs() {
            best = i;
        }
    }
    best
}

impl Settings {
    pub fn load(c: &Config) -> Self {
        let mut s = Settings { display: DisplayConfig::from_config(c), ..Default::default() };
        s.caps.mask(&mut s.display);
        s.fps_custom = c.get_or("display.fps_custom", s.fps_custom).clamp(1.0, 1000.0);
        s.hide_names = c.get_bool("ui.hide_names", false);
        s.input_guide = c.get("ui.input_guide").and_then(super::guide::Policy::parse).unwrap_or_default();
        s.perf_overlay = c.get_bool("ui.perf_overlay", s.perf_overlay);
        s.sprite_cache = c.get_bool("gfx.sprite_cache", s.sprite_cache);
        s.analytic_shapes = c.get_bool("gfx.analytic_shapes", s.analytic_shapes);
        let b = Bindings::from_config(c);
        s.mouse_steering = b.mouse_steering;
        s.dead_zone = b.dead_zone;
        s.touch_scale = b.touch_scale;
        if let Some(n) = c.get("input.touch_aim").and_then(|v| TOUCH_AIMS.iter().position(|(_, k)| k.eq_ignore_ascii_case(v))) {
            s.touch_aim = n as u8;
        }
        for (i, key) in MOUSE_KEYS.iter().enumerate() {
            if let Some(v) = c.get(key)
                && let Some(n) = MOUSE_ACTIONS.iter().position(|(name, _)| name.eq_ignore_ascii_case(v))
            {
                s.mouse[i] = n as u8;
            }
        }
        s
    }

    pub fn store(&self, c: &mut Config) {
        self.display.store(c);
        c.set("display.fps_custom", self.fps_custom);
        c.set("ui.input_guide", self.input_guide.name());
        c.set("ui.hide_names", self.hide_names);
        c.set("ui.perf_overlay", if self.perf_overlay { "on" } else { "off" });
        c.set("gfx.sprite_cache", if self.sprite_cache { "on" } else { "off" });
        c.set("gfx.analytic_shapes", if self.analytic_shapes { "on" } else { "off" });
        for (i, key) in MOUSE_KEYS.iter().enumerate() {
            c.set(key, MOUSE_ACTIONS[self.mouse[i] as usize].0.to_ascii_lowercase());
        }
        c.set("input.mouse_steering", self.mouse_steering);
        c.set("input.dead_zone", self.dead_zone);
        c.set("input.touch_scale", self.touch_scale);
        c.set("input.touch_aim", TOUCH_AIMS[self.touch_aim as usize].1);
    }

    /// The bindings as these settings tune them: the mouse buttons'
    /// actions replace the mouse sources.
    pub fn apply_input(&self, b: &mut Bindings) {
        b.binds.retain(|(s, _)| !matches!(s, Source::Mouse(_)));
        for (button, &n) in self.mouse.iter().enumerate() {
            if let Some(action) = MOUSE_ACTIONS[n as usize].1 {
                b.binds.push((Source::Mouse(button as u8), action));
            }
        }
        b.mouse_steering = self.mouse_steering;
        b.dead_zone = self.dead_zone;
        b.touch_scale = self.touch_scale;
    }

    /// The FPS cap as an index of the options' list: the presets, the
    /// display's refresh rate (MAX), then CUSTOM.
    pub fn fps_index(&self) -> usize {
        if let Some(i) = FpsCap::PRESETS.iter().position(|p| *p == self.display.fps_cap) {
            return i;
        }
        if Some(self.display.fps_cap) == max_refresh_cap() {
            return FpsCap::PRESETS.len();
        }
        FpsCap::PRESETS.len() + 1
    }

    pub fn set_fps_index(&mut self, i: usize) {
        self.display.fps_cap = match FpsCap::PRESETS.get(i) {
            Some(&cap) => cap,
            None if i == FpsCap::PRESETS.len() => max_refresh_cap().unwrap_or(FpsCap::VSync),
            None => FpsCap::Fps(self.fps_custom),
        };
    }

    /// Takes another platform's capabilities (`--caps`): the display
    /// settings they hide go back to the defaults.
    pub fn set_caps(&mut self, caps: Capabilities) {
        self.caps = caps;
        caps.mask(&mut self.display);
    }

    pub fn resolution_index(&self) -> usize {
        let d = &self.display;
        RESOLUTIONS.iter().position(|&r| r == (d.width, d.height)).unwrap_or(RESOLUTIONS.len())
    }

    pub fn window_mode_index(&self) -> usize {
        match self.display.mode {
            WindowMode::Windowed => 0,
            WindowMode::Borderless => 1,
            WindowMode::Fullscreen => 2,
        }
    }
}

pub const WINDOW_MODES: [(&str, WindowMode); 3] =
    [("WINDOWED", WindowMode::Windowed), ("BORDERLESS", WindowMode::Borderless), ("FULLSCREEN", WindowMode::Fullscreen)];

/// The menus' view of the devices: edges and pointer hits made from the
/// engine's actions each tick (display only; never in a .rec).
#[derive(Clone, Debug, Default)]
pub struct UiInput {
    /// The device used last (the prompts follow it).
    pub device: Device,
    /// Previous (-1) or next (+1) tab asked this tick: Q/E, PageUp and
    /// PageDown, the shoulder buttons, the wheel over the tab strip, a
    /// sideways swipe.
    pub tab: i8,
    /// What asked for `tab` (a key or button being remapped wins over it).
    pub tab_by: TabBy,
    /// The mouse or a finger, in arena units.
    pub cursor: Option<[f32; 2]>,
    /// The cursor moved this tick.
    pub moved: bool,
    /// A click (mouse left press) or tap (a short touch) this tick, where.
    pub click: Option<[f32; 2]>,
    /// Wheel notches this tick (up positive).
    pub wheel: f32,
    /// The device the menus show their pointer targets for, when forced
    /// (screenshots); else `device`.
    pub shown: Option<Device>,
    /// A mouse right press this tick, where.
    pub right_click: Option<[f32; 2]>,
    /// The mouse's back button (X1) was pressed this tick: cancel.
    pub back: bool,
    /// A step through a list asked this tick by the wheel (one notch) or a
    /// vertical swipe: -1 up, +1 down.
    pub nav: i8,
    held: u32,
    wheel_total: f32,
    touch_start: Option<([f32; 2], u32)>,
    touch_last: Option<[f32; 2]>,
    ticks: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabBy {
    #[default]
    None,
    /// Q or E.
    Letter,
    /// PageUp or PageDown.
    Page,
    /// A shoulder button.
    Shoulder,
    /// The wheel, a click or a swipe.
    Pointer,
}

const UI_Q: u32 = 1;
const UI_E: u32 = 2;
const UI_PGUP: u32 = 4;
const UI_PGDN: u32 = 8;
const UI_LB: u32 = 16;
const UI_RB: u32 = 32;
const UI_CLICK: u32 = 64;
const UI_RIGHT: u32 = 128;
const UI_BACK: u32 = 256;

impl UiInput {
    /// The pointer is the device in use (the mouse or a finger).
    pub fn pointing(&self) -> bool {
        matches!(self.device, Device::Mouse | Device::Touch)
    }

    /// The menus show their pointer targets (arrows, BACK).
    pub fn shows_pointer(&self) -> bool {
        matches!(self.shown.unwrap_or(self.device), Device::Mouse | Device::Touch)
    }

    pub fn update(&mut self, a: &wf_engine::input::ActionState) {
        use wf_engine::platform::{Key, PadButton};
        self.ticks = self.ticks.wrapping_add(1);
        let raw = &a.raw;
        let pad = |b: PadButton| raw.gamepads.iter().any(|g| g.held(b));
        let mut held = 0;
        for (on, bit) in [
            (raw.key(Key::Q), UI_Q),
            (raw.key(Key::E), UI_E),
            (raw.key(Key::PageUp), UI_PGUP),
            (raw.key(Key::PageDown), UI_PGDN),
            (pad(PadButton::LeftShoulder), UI_LB),
            (pad(PadButton::RightShoulder), UI_RB),
            (raw.mouse.buttons & 1 != 0, UI_CLICK),
            (raw.mouse.buttons & 4 != 0, UI_RIGHT),
            (raw.mouse.buttons & 8 != 0, UI_BACK),
        ] {
            if on {
                held |= bit;
            }
        }
        let pressed = held & !self.held;
        self.held = held;
        self.device = a.device;
        self.tab = 0;
        self.tab_by = TabBy::None;
        for (bit, dir, by) in [
            (UI_Q, -1, TabBy::Letter),
            (UI_E, 1, TabBy::Letter),
            (UI_PGUP, -1, TabBy::Page),
            (UI_PGDN, 1, TabBy::Page),
            (UI_LB, -1, TabBy::Shoulder),
            (UI_RB, 1, TabBy::Shoulder),
        ] {
            if pressed & bit != 0 {
                self.tab = dir;
                self.tab_by = by;
            }
        }
        self.wheel = raw.mouse.wheel - self.wheel_total;
        self.wheel_total = raw.mouse.wheel;
        let last = self.cursor;
        self.cursor = a.touches.first().copied().or(a.cursor);
        self.moved = last.is_some() && self.cursor.is_some() && last != self.cursor;
        self.click = None;
        if pressed & UI_CLICK != 0 {
            self.click = a.cursor;
        }
        self.right_click = if pressed & UI_RIGHT != 0 { a.cursor } else { None };
        self.back = pressed & UI_BACK != 0;
        self.nav = if self.wheel > 0.0 {
            -1
        } else if self.wheel < 0.0 {
            1
        } else {
            0
        };
        // Touch: a short touch that stays put is a tap; a quick sideways
        // move is a swipe.
        match (a.touches.first(), self.touch_start) {
            (Some(&p), None) => self.touch_start = Some((p, self.ticks)),
            (Some(&p), Some(_)) => self.touch_last = Some(p),
            (None, Some((start, at))) => {
                let end = self.touch_last.unwrap_or(start);
                let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
                let quick = self.ticks.wrapping_sub(at) < 30;
                if quick && dx.abs() > 80.0 && dx.abs() > dy.abs() * 2.0 {
                    self.tab = if dx < 0.0 { 1 } else { -1 };
                    self.tab_by = TabBy::Pointer;
                } else if quick && dy.abs() > 40.0 && dy.abs() > dx.abs() * 2.0 {
                    // Swiping up shows the next item, as a list scrolls.
                    self.nav = if dy < 0.0 { 1 } else { -1 };
                } else if quick && dx.abs() + dy.abs() < 12.0 {
                    self.click = Some(start);
                }
                self.touch_start = None;
                self.touch_last = None;
            }
            (None, None) => {}
        }
    }
}

/// The options' MAX FPS cap: the display's refresh rate (rounded to
/// hundredths, e.g. 165 or 59.94), when the platform knows it.
pub fn max_refresh_cap() -> Option<FpsCap> {
    wf_engine::platform::max_refresh().map(|hz| FpsCap::Fps((hz as f64 * 100.0).round() / 100.0))
}
