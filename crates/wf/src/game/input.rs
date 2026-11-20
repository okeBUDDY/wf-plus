//! The pads (HLInput / HLInpDev): eight directions and eight logical
//! triggers per pad, built from the keyboard and SDL gamepads each frame.

use crate::platform::{Key, PadButton, RawInput};
use wf_engine::input::{Action, ActionState};

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

/// The ship's speed per tick (PlayerControl::OnMove): normal and slow.
const SHIP_SPEED: [f32; 2] = [7.0, 2.0];

/// What the pointer and drag quantizing needs: the ship, and the drag not
/// yet turned into moves.
#[derive(Clone, Copy, Debug, Default)]
pub struct Steer {
    /// The player's ship (arena units) when one is in play.
    pub ship: Option<[f32; 2]>,
    residual: [f32; 2],
}

/// The eight-way direction bits of `v` (any length), or none when it is
/// shorter than `min`.
fn eight_way(v: [f32; 2], min: f32) -> u16 {
    let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if len <= min || len == 0.0 {
        return 0;
    }
    // sin(22.5 deg): the edge between a straight and a diagonal sector.
    const EDGE: f32 = 0.382_683_43;
    let (x, y) = (v[0] / len, v[1] / len);
    let mut b = 0;
    if x < -EDGE {
        b |= LEFT;
    } else if x > EDGE {
        b |= RIGHT;
    }
    if y < -EDGE {
        b |= UP;
    } else if y > EDGE {
        b |= DOWN;
    }
    b
}

/// The direction bits of the actions: the move vector, else the pointer
/// (the ship steers toward it, stopping within a step), else a touch drag
/// (spent a ship's step at a time).
pub fn quantize(a: &ActionState, steer: &mut Steer) -> u16 {
    let axis = eight_way(a.axis, 0.0);
    if axis != 0 {
        steer.residual = [0.0; 2];
        return axis;
    }
    let step = SHIP_SPEED[a.held(Action::SLOW) as usize];
    let Some(ship) = steer.ship else {
        steer.residual = [0.0; 2];
        return 0;
    };
    if let Some(p) = a.pointer {
        return eight_way([p[0] - ship[0], p[1] - ship[1]], step * 0.5);
    }
    steer.residual[0] += a.drag[0];
    steer.residual[1] += a.drag[1];
    let b = eight_way(steer.residual, step * 0.5);
    if b != 0 {
        let d = std::f32::consts::FRAC_1_SQRT_2;
        let diag = (b & (LEFT | RIGHT) != 0) && (b & (UP | DOWN) != 0);
        let k = if diag { step * d } else { step };
        let dx = if b & LEFT != 0 { -k } else if b & RIGHT != 0 { k } else { 0.0 };
        let dy = if b & UP != 0 { -k } else if b & DOWN != 0 { k } else { 0.0 };
        steer.residual = [steer.residual[0] - dx, steer.residual[1] - dy];
    }
    if !a.touching {
        steer.residual = [0.0; 2];
    }
    b
}

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
    /// HLKeyboard::Poll (0x405c00) and HLJoystick::Poll (0x405ab0), now fed
    /// by the engine's actions: whatever device made them, they reduce to
    /// the original's eight directions and eight triggers, the bits a .rec
    /// stores. Analog and pointer input is quantized here, before the
    /// game sees it (see [`quantize`]).
    pub fn poll(&mut self, a: &ActionState, steer: &mut Steer) {
        let mut b = quantize(a, steer);
        for i in 0..self.trigger_count {
            if a.held(Action::Trigger(i as u8)) {
                b |= TRIGGER << i;
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
    /// The pointer and drag quantizing (not game state: a .rec keeps only
    /// the bits it makes).
    pub steer: Steer,
}

impl Input {
    /// Reads this tick's actions (the window must be active; see
    /// InputTask::IsProtected).
    pub fn poll(&mut self, a: &ActionState) {
        for pad in &mut self.pads {
            pad.poll(a, &mut self.steer);
        }
        let raw = &a.raw;
        let quit = a.held(Action::Quit);
        let reset = a.held(Action::Reset);
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
