//! The pads (HLInput / HLInpDev): eight directions and eight logical
//! triggers per pad, built from the keyboard and SDL gamepads each frame.

use crate::platform::{Key, PadButton, RawInput};

pub const UP: u16 = 0x1;
pub const DOWN: u16 = 0x2;
pub const LEFT: u16 = 0x4;
pub const RIGHT: u16 = 0x8;
/// Logical trigger n is `TRIGGER << n`.
pub const TRIGGER: u16 = 0x100;
pub const MAX_TRIGGERS: usize = 8;

/// Trigger 0: shot, 1: slow move, 3: weapon select, 7: pause.
pub const SHOT: u16 = TRIGGER;
pub const SLOW: u16 = TRIGGER << 1;
pub const WEAPON_SELECT: u16 = TRIGGER << 3;
pub const PAUSE: u16 = TRIGGER << 7;

/// The keys a keyboard trigger can be assigned to, in the order of the
/// original's table (0x45d1d8); also the default keyboard map.
pub const TRIGGER_KEYS: [Key; MAX_TRIGGERS] = [Key::Z, Key::X, Key::S, Key::D, Key::A, Key::W, Key::E, Key::F];

/// Marks a gamepad button in a trigger code.
pub const PAD_CODE: i32 = 0x80;

/// Stick deflection that counts as a direction.
const DEAD_ZONE: f32 = 0.35;

pub struct Pad {
    pub buttons: u16,
    pub pressed: u16,
    previous: u16,
    /// Keyboard key of each logical trigger.
    pub key_map: [Key; MAX_TRIGGERS],
    /// Gamepad button (index into `PadButton::TRIGGERS`) of each trigger.
    pub button_map: [u8; MAX_TRIGGERS],
    pub trigger_count: usize,
}

impl Default for Pad {
    fn default() -> Self {
        Pad {
            buttons: 0,
            pressed: 0,
            previous: 0,
            key_map: TRIGGER_KEYS,
            button_map: [0, 1, 2, 3, 4, 5, 6, 7],
            trigger_count: MAX_TRIGGERS,
        }
    }
}

impl Pad {
    /// HLKeyboard::Poll (0x405c00) and HLJoystick::Poll (0x405ab0) merged:
    /// the keyboard and every gamepad drive the pad together.
    pub fn poll(&mut self, raw: &RawInput) {
        let mut b = 0u16;
        let k = |key| raw.key(key);
        if k(Key::Numpad2) || k(Key::Down) {
            b |= DOWN;
        }
        if k(Key::Numpad4) || k(Key::Left) {
            b |= LEFT;
        }
        if k(Key::Numpad6) || k(Key::Right) {
            b |= RIGHT;
        }
        if k(Key::Numpad8) || k(Key::Up) {
            b |= UP;
        }
        if k(Key::Numpad1) {
            b |= DOWN | LEFT;
        }
        if k(Key::Numpad3) {
            b |= DOWN | RIGHT;
        }
        if k(Key::Numpad7) {
            b |= UP | LEFT;
        }
        if k(Key::Numpad9) {
            b |= UP | RIGHT;
        }
        for i in 0..self.trigger_count {
            if k(self.key_map[i]) {
                b |= TRIGGER << i;
            }
        }
        for g in &raw.gamepads {
            if g.stick[1] < -DEAD_ZONE || g.held(PadButton::DpadUp) {
                b |= UP;
            }
            if g.stick[1] > DEAD_ZONE || g.held(PadButton::DpadDown) {
                b |= DOWN;
            }
            if g.stick[0] < -DEAD_ZONE || g.held(PadButton::DpadLeft) {
                b |= LEFT;
            }
            if g.stick[0] > DEAD_ZONE || g.held(PadButton::DpadRight) {
                b |= RIGHT;
            }
            for i in 0..self.trigger_count {
                if let Some(&button) = PadButton::TRIGGERS.get(self.button_map[i] as usize)
                    && g.held(button) {
                        b |= TRIGGER << i;
                    }
            }
        }
        self.buttons = b;
        self.pressed = b & !self.previous;
        self.previous = b;
    }

    /// HLInpDev::trigger: the device code of trigger `index` (0 past the
    /// last). Keyboard keys are their index in `TRIGGER_KEYS`; gamepad
    /// buttons are `PAD_CODE | button`.
    pub fn trigger(&self, index: usize) -> i32 {
        if index >= MAX_TRIGGERS {
            return 0;
        }
        TRIGGER_KEYS.iter().position(|&k| k == self.key_map[index]).unwrap_or(0) as i32
    }

    /// HLInpDev::set_trigger.
    pub fn set_trigger(&mut self, index: usize, code: i32) {
        if index >= MAX_TRIGGERS {
            return;
        }
        if code & PAD_CODE != 0 {
            self.button_map[index] = (code & 0x7f) as u8;
        } else if let Some(&k) = TRIGGER_KEYS.get(code as usize) {
            self.key_map[index] = k;
        }
    }

    /// GetPressed: the code of a trigger key or button held now, if any.
    pub fn get_pressed(&self, raw: &RawInput) -> Option<i32> {
        if let Some(i) = TRIGGER_KEYS.iter().position(|&k| raw.key(k)) {
            return Some(i as i32);
        }
        for g in &raw.gamepads {
            if let Some(b) = PadButton::TRIGGERS.iter().position(|&b| g.held(b)) {
                return Some(PAD_CODE | b as i32);
            }
        }
        None
    }

    /// GetName: two characters naming device code `code`.
    pub fn name(code: i32) -> [u8; 2] {
        if code & PAD_CODE != 0 {
            let n = format!("{:<2}", (code & 0x7f) + 1);
            let b = n.as_bytes();
            return [b[0], b[1]];
        }
        const NAMES: [u8; MAX_TRIGGERS] = *b"ZXSDAWEF";
        [NAMES.get(code as usize).copied().unwrap_or(b' '), b' ']
    }

    /// The trigger maps as the profile keeps them ("%d %d ..."), for the
    /// keyboard and the gamepads.
    pub fn trigger_maps(&self) -> (String, String) {
        let keys: Vec<String> = (0..MAX_TRIGGERS).map(|i| self.trigger(i).to_string()).collect();
        let buttons: Vec<String> = self.button_map.iter().map(|b| b.to_string()).collect();
        (keys.join(" "), buttons.join(" "))
    }

    /// HLInpDev::LoadTriggerMap: reads the maps written by `trigger_maps`.
    pub fn set_trigger_maps(&mut self, keys: &str, buttons: &str) {
        for (i, v) in keys.split_whitespace().take(MAX_TRIGGERS).enumerate() {
            if let Ok(c) = v.parse::<i32>()
                && (0..MAX_TRIGGERS as i32).contains(&c) {
                    self.set_trigger(i, c);
                }
        }
        for (i, v) in buttons.split_whitespace().take(MAX_TRIGGERS).enumerate() {
            if let Ok(b) = v.parse::<u8>()
                && (b as usize) < PadButton::TRIGGERS.len() {
                    self.button_map[i] = b;
                }
        }
    }

    /// The pad as a replay would feed it.
    pub fn set(&mut self, buttons: u16) {
        self.buttons = buttons;
        self.pressed = buttons & !self.previous;
        self.previous = buttons;
    }
}

/// The game's input (one pad, as WinMain sets it up).
#[derive(Default)]
pub struct Input {
    pub pads: [Pad; 1],
    /// Escape and F12 (the system keys), pressed this frame.
    pub quit_pressed: bool,
    pub reset_pressed: bool,
    /// Buttons of pad 0 that stop and restart the time (HLInput's
    /// time_stop_mask_).
    pub time_stop_mask: u16,
    /// The devices as polled last, for GetPressed.
    pub raw: RawInput,
    held_quit: bool,
    held_reset: bool,
}

impl Input {
    pub fn poll(&mut self, raw: &RawInput) {
        for pad in &mut self.pads {
            pad.poll(raw);
        }
        let quit = raw.key(Key::Escape);
        let reset = raw.key(Key::F12);
        self.quit_pressed = quit && !self.held_quit;
        self.reset_pressed = reset && !self.held_reset;
        self.held_quit = quit;
        self.held_reset = reset;
        self.raw = raw.clone();
    }

    /// GetPressed of pad 0.
    pub fn get_pressed(&self) -> Option<i32> {
        self.pads[0].get_pressed(&self.raw)
    }

    pub fn pad(&self, i: usize) -> &Pad {
        &self.pads[i]
    }
}
