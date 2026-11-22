//! Action-based input. Device providers (keyboard, mouse, gamepads, touch
//! screens and gamepad touchpads) feed one [`ActionState`]: the game reads
//! abstract actions and a move vector and never knows which device made
//! them. Bindings are data ([`Bindings`]), loaded from the config's
//! `input` section and changed by the game's options screen.

use crate::config::Config;
use crate::gfx::Layout;
use crate::platform::{Device, Key, PadButton, RawInput};

/// An abstract action. The eight triggers are the original's logical
/// pad triggers: 0 shot (and menu confirm), 1 slow move / laser (and menu
/// cancel), 3 weapon select, 7 pause; 2, 4, 5, 6 are spare.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Trigger(u8),
    /// Leave the game (Escape).
    Quit,
    /// Back to the title (F12).
    Reset,
}

impl Action {
    pub const SHOT: Action = Action::Trigger(0);
    pub const SLOW: Action = Action::Trigger(1);
    pub const WEAPON: Action = Action::Trigger(3);
    pub const PAUSE: Action = Action::Trigger(7);
    pub const CONFIRM: Action = Action::SHOT;
    pub const CANCEL: Action = Action::SLOW;

    fn bit(self) -> u32 {
        match self {
            Action::Up => 1,
            Action::Down => 2,
            Action::Left => 4,
            Action::Right => 8,
            Action::Trigger(n) => 0x100 << (n & 7),
            Action::Quit => 0x1_0000,
            Action::Reset => 0x2_0000,
        }
    }

    pub fn parse(s: &str) -> Option<Action> {
        Some(match s {
            "up" => Action::Up,
            "down" => Action::Down,
            "left" => Action::Left,
            "right" => Action::Right,
            "shot" | "confirm" => Action::SHOT,
            "slow" | "laser" | "cancel" => Action::SLOW,
            "weapon" => Action::WEAPON,
            "pause" => Action::PAUSE,
            "quit" => Action::Quit,
            "reset" => Action::Reset,
            t => Action::Trigger(t.strip_prefix("trigger")?.parse().ok().filter(|n: &u8| *n < 8)?),
        })
    }
}

/// A physical input a binding names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Key(Key),
    Pad(PadButton),
    /// Mouse button 0 left, 1 middle, 2 right.
    Mouse(u8),
    /// A finger on the screen or a gamepad touchpad.
    Touch,
}

impl Source {
    /// "key:z", "pad:south", "mouse:left", "touch".
    pub fn parse(s: &str) -> Option<Source> {
        let s = s.trim().to_ascii_lowercase();
        if s == "touch" {
            return Some(Source::Touch);
        }
        let (kind, name) = s.split_once(':')?;
        match kind {
            "key" => Key::parse(name).map(Source::Key),
            "pad" => PadButton::parse(name).map(Source::Pad),
            "mouse" => match name {
                "left" => Some(Source::Mouse(0)),
                "middle" => Some(Source::Mouse(1)),
                "right" => Some(Source::Mouse(2)),
                _ => None,
            },
            _ => None,
        }
    }
}

/// What drives each action.
#[derive(Clone, Debug)]
pub struct Bindings {
    pub binds: Vec<(Source, Action)>,
    /// Stick deflection that counts.
    pub dead_zone: f32,
    pub mouse_steering: bool,
    pub touch: bool,
    /// Touch drag speed (1 = the finger's own travel).
    pub touch_scale: f32,
}

impl Default for Bindings {
    fn default() -> Self {
        use Action as A;
        use Key as K;
        use PadButton as P;
        let mut binds = vec![
            (Source::Key(K::Up), A::Up),
            (Source::Key(K::Down), A::Down),
            (Source::Key(K::Left), A::Left),
            (Source::Key(K::Right), A::Right),
            (Source::Key(K::Escape), A::Quit),
            (Source::Key(K::F12), A::Reset),
            (Source::Pad(P::DpadUp), A::Up),
            (Source::Pad(P::DpadDown), A::Down),
            (Source::Pad(P::DpadLeft), A::Left),
            (Source::Pad(P::DpadRight), A::Right),
            (Source::Mouse(0), A::SHOT),
            (Source::Mouse(2), A::SLOW),
            (Source::Mouse(1), A::WEAPON),
            (Source::Touch, A::SHOT),
        ];
        // The original's default trigger maps: ZXSDAWEF and pad buttons 1-8.
        binds.extend(DEFAULT_TRIGGER_KEYS.iter().enumerate().map(|(i, &k)| (Source::Key(k), A::Trigger(i as u8))));
        binds.extend(PadButton::TRIGGERS[..8].iter().enumerate().map(|(i, &b)| (Source::Pad(b), A::Trigger(i as u8))));
        Bindings { binds, dead_zone: 0.35, mouse_steering: true, touch: true, touch_scale: 1.0 }
    }
}

/// The keys of triggers 0..8 by default (the original's table, 0x45d1d8).
pub const DEFAULT_TRIGGER_KEYS: [Key; 8] = [Key::Z, Key::X, Key::S, Key::D, Key::A, Key::W, Key::E, Key::F];

impl Bindings {
    /// The defaults, then the config's `input` section: `bind.<action> =
    /// source, source, ...` replaces an action's bindings; `dead_zone`,
    /// `mouse_steering` and `touch` tune the providers.
    pub fn from_config(c: &Config) -> Self {
        let mut b = Bindings::default();
        for (k, v) in c.section("input") {
            if let Some(name) = k.strip_prefix("bind.")
                && let Some(action) = Action::parse(name)
            {
                b.binds.retain(|(_, a)| *a != action);
                b.binds.extend(v.split(',').filter_map(Source::parse).map(|s| (s, action)));
            }
        }
        b.dead_zone = c.get_or("input.dead_zone", b.dead_zone).clamp(0.05, 0.95);
        b.mouse_steering = c.get_bool("input.mouse_steering", b.mouse_steering);
        b.touch = c.get_bool("input.touch", b.touch);
        b.touch_scale = c.get_or("input.touch_scale", b.touch_scale).clamp(0.25, 4.0);
        b
    }

    /// Rebinds trigger `n`'s keyboard key (the options screen's remap).
    pub fn set_trigger_key(&mut self, n: u8, key: Key) {
        self.binds.retain(|(s, a)| !(matches!(s, Source::Key(_)) && *a == Action::Trigger(n)));
        self.binds.push((Source::Key(key), Action::Trigger(n)));
    }

    /// Rebinds trigger `n`'s gamepad button.
    pub fn set_trigger_button(&mut self, n: u8, button: PadButton) {
        self.binds.retain(|(s, a)| !(matches!(s, Source::Pad(_)) && *a == Action::Trigger(n)));
        self.binds.push((Source::Pad(button), Action::Trigger(n)));
    }
}

/// The actions this frame, whatever made them.
#[derive(Clone, Debug, Default)]
pub struct ActionState {
    held: u32,
    /// The actions held by touch alone (no other source holds them).
    touch_only: u32,
    /// The move vector, -1..1 each way, y down: digital directions give
    /// ±1, sticks their deflection past the dead zone.
    pub axis: [f32; 2],
    /// The mouse in arena units, when the mouse is the last-used device
    /// and steering is on: the ship steers toward it.
    pub pointer: Option<[f32; 2]>,
    /// A relative drag since the last frame (touch screens and gamepad
    /// touchpads), in arena units.
    pub drag: [f32; 2],
    /// Some finger is down.
    pub touching: bool,
    /// The finger that steers (the first one down; the next oldest when
    /// it lifts). Only its travel makes `drag`.
    pub primary: Option<u64>,
    /// The mouse in arena units whenever it is in the window (menus).
    pub cursor: Option<[f32; 2]>,
    /// The fingers down, in arena units (menus).
    pub touches: Vec<[f32; 2]>,
    /// The device used last; it picks the UI's prompts.
    pub device: Device,
    /// The devices as read, for the options screen's "press a button".
    pub raw: RawInput,
}

impl ActionState {
    pub fn held(&self, a: Action) -> bool {
        self.held & a.bit() != 0
    }

    /// Holds `a` as a finger would (tests and scripted input).
    pub fn hold_by_touch(&mut self, a: Action) {
        self.held |= a.bit();
        self.touch_only |= a.bit();
    }

    /// Held, and only by a finger: the game may read the finger otherwise.
    pub fn held_by_touch_only(&self, a: Action) -> bool {
        self.touch_only & a.bit() != 0
    }
}

/// Turns devices into actions.
#[derive(Default)]
pub struct InputSystem {
    pub bindings: Bindings,
    last_fingers: Vec<(u64, [f32; 2])>,
    primary: Option<u64>,
    last_pads: Vec<Option<[f32; 2]>>,
}

impl InputSystem {
    pub fn new(bindings: Bindings) -> Self {
        InputSystem { bindings, ..Default::default() }
    }

    /// The actions of `raw`; `layout` maps window pixels to arena units.
    pub fn update(&mut self, raw: &RawInput, layout: &Layout) -> ActionState {
        let mut s = ActionState { device: raw.last_device, raw: raw.clone(), ..Default::default() };
        let source_held = |src: &Source| match *src {
            Source::Key(k) => raw.key(k),
            Source::Pad(b) => raw.gamepads.iter().any(|g| g.held(b)),
            Source::Mouse(m) => raw.mouse.buttons & (1 << m) != 0,
            Source::Touch => self.bindings.touch && !raw.fingers.is_empty(),
        };
        let (mut touch, mut other) = (0, 0);
        for (src, action) in &self.bindings.binds {
            if source_held(src) {
                if *src == Source::Touch {
                    touch |= action.bit();
                } else {
                    other |= action.bit();
                }
            }
        }
        s.held = touch | other;
        s.touch_only = touch & !other;
        // The digital directions, then the sticks.
        let mut axis = [0.0f32; 2];
        if s.held(Action::Left) {
            axis[0] -= 1.0;
        }
        if s.held(Action::Right) {
            axis[0] += 1.0;
        }
        if s.held(Action::Up) {
            axis[1] -= 1.0;
        }
        if s.held(Action::Down) {
            axis[1] += 1.0;
        }
        let dz = self.bindings.dead_zone;
        for g in &raw.gamepads {
            let [x, y] = g.stick;
            if (x * x + y * y).sqrt() > dz {
                axis[0] += x;
                axis[1] += y;
            }
        }
        s.axis = [axis[0].clamp(-1.0, 1.0), axis[1].clamp(-1.0, 1.0)];
        // The mouse steers when it was used last.
        if self.bindings.mouse_steering && raw.last_device == Device::Mouse && raw.mouse.inside {
            s.pointer = Some(layout.to_logical(raw.mouse.pos[0], raw.mouse.pos[1]));
        }
        // Touch: a relative drag of any finger that stayed down.
        let (w, h) = (raw.window.0.max(1) as f32, raw.window.1.max(1) as f32);
        if raw.mouse.inside {
            s.cursor = Some(layout.to_logical(raw.mouse.pos[0], raw.mouse.pos[1]));
        }
        s.touches = raw.fingers.iter().map(|f| layout.to_logical(f.pos[0] * w, f.pos[1] * h)).collect();
        if !raw.fingers.iter().any(|f| Some(f.id) == self.primary) {
            // The oldest finger left (fingers come in the order they went down).
            self.primary = raw.fingers.first().map(|f| f.id);
        }
        s.primary = self.primary;
        if self.bindings.touch {
            for f in raw.fingers.iter().filter(|f| Some(f.id) == self.primary) {
                if let Some((_, p)) = self.last_fingers.iter().find(|(id, _)| *id == f.id) {
                    s.drag[0] += (f.pos[0] - p[0]) * w / layout.scale;
                    s.drag[1] += (f.pos[1] - p[1]) * h / layout.scale;
                }
            }
            // A gamepad touchpad drags across the arena's width.
            for (i, g) in raw.gamepads.iter().enumerate() {
                if let (Some(now), Some(Some(before))) = (g.touchpad, self.last_pads.get(i)) {
                    s.drag[0] += (now[0] - before[0]) * layout.visible[2];
                    s.drag[1] += (now[1] - before[1]) * layout.visible[2] * 0.5;
                }
                if g.touchpad.is_some() {
                    s.touching = true;
                }
            }
            s.touching |= !raw.fingers.is_empty();
            let k = self.bindings.touch_scale;
            s.drag = [s.drag[0] * k, s.drag[1] * k];
        }
        self.last_fingers = raw.fingers.iter().map(|f| (f.id, f.pos)).collect();
        self.last_pads = raw.gamepads.iter().map(|g| g.touchpad).collect();
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{Finger, GamepadState, MouseState};

    fn layout() -> Layout {
        Layout::new([640.0, 480.0], (1280, 720))
    }

    #[test]
    fn every_device_makes_the_same_actions() {
        let mut sys = InputSystem::default();
        let mut raw = RawInput::default();
        raw.keys[Key::Z as usize] = true;
        assert!(sys.update(&raw, &layout()).held(Action::SHOT));
        let mut raw = RawInput::default();
        raw.gamepads.push(GamepadState { buttons: 1 << PadButton::South as u32, ..Default::default() });
        assert!(sys.update(&raw, &layout()).held(Action::SHOT));
        let raw = RawInput { mouse: MouseState { buttons: 1, ..Default::default() }, ..Default::default() };
        assert!(sys.update(&raw, &layout()).held(Action::SHOT));
    }

    #[test]
    fn sticks_have_a_dead_zone() {
        let mut sys = InputSystem::default();
        let mut raw = RawInput::default();
        raw.gamepads.push(GamepadState { stick: [0.2, 0.0], ..Default::default() });
        assert_eq!(sys.update(&raw, &layout()).axis, [0.0, 0.0]);
        raw.gamepads[0].stick = [0.8, -0.5];
        assert_eq!(sys.update(&raw, &layout()).axis, [0.8, -0.5]);
    }

    #[test]
    fn touch_drags_in_arena_units() {
        let mut sys = InputSystem::default();
        let mut raw = RawInput { window: (1280, 720), ..Default::default() };
        raw.fingers.push(Finger { id: 7, pos: [0.5, 0.5] });
        let s = sys.update(&raw, &layout());
        assert!(s.touching && s.held(Action::SHOT) && s.held_by_touch_only(Action::SHOT));
        raw.keys[Key::Z as usize] = true;
        assert!(!sys.update(&raw, &layout()).held_by_touch_only(Action::SHOT));
        raw.keys[Key::Z as usize] = false;
        sys.update(&raw, &layout());
        raw.fingers[0].pos = [0.5 + 15.0 / 1280.0, 0.5];
        let s = sys.update(&raw, &layout());
        assert!((s.drag[0] - 10.0).abs() < 1e-3, "{:?}", s.drag);
    }

    #[test]
    fn only_the_first_finger_drags() {
        let mut sys = InputSystem::default();
        let mut raw = RawInput { window: (1280, 720), ..Default::default() };
        raw.fingers.push(Finger { id: 1, pos: [0.5, 0.5] });
        sys.update(&raw, &layout());
        raw.fingers.push(Finger { id: 2, pos: [0.8, 0.5] });
        assert_eq!(sys.update(&raw, &layout()).primary, Some(1));
        // The second finger moves: no drag.
        raw.fingers[1].pos = [0.9, 0.6];
        assert_eq!(sys.update(&raw, &layout()).drag, [0.0, 0.0]);
        // The first lifts: the second steers, from where it is.
        raw.fingers.remove(0);
        let s = sys.update(&raw, &layout());
        assert_eq!((s.primary, s.drag), (Some(2), [0.0, 0.0]));
        raw.fingers[0].pos = [0.9 + 15.0 / 1280.0, 0.6];
        assert!((sys.update(&raw, &layout()).drag[0] - 10.0).abs() < 1e-3);
    }

    #[test]
    fn bindings_come_from_config() {
        let mut c = Config::default();
        c.parse("[input]\nbind.shot = key:space, mouse:right\n");
        let b = Bindings::from_config(&c);
        assert!(b.binds.contains(&(Source::Key(Key::Space), Action::SHOT)));
        assert!(!b.binds.contains(&(Source::Key(Key::Z), Action::SHOT)));
    }
}
