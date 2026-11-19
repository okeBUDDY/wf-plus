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
                if let Some(&button) = PadButton::TRIGGERS.get(self.button_map[i] as usize) {
                    if g.held(button) {
                        b |= TRIGGER << i;
                    }
                }
            }
        }
        self.buttons = b;
        self.pressed = b & !self.previous;
        self.previous = b;
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
    }

    pub fn pad(&self, i: usize) -> &Pad {
        &self.pads[i]
    }
}
