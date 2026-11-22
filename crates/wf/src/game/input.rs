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
    /// Touch without a ship (see [`menu_touch`]): the swipe not yet
    /// turned into steps, the finger's travel and ticks down.
    swipe: [f32; 2],
    travel: f32,
    touch_ticks: u32,
    was_touching: bool,
    /// In play, a second finger (see [`play_extras`]): its id, ticks down,
    /// travel and last place.
    second: Option<(u64, u32, f32, [f32; 2])>,
    /// The mouse wheel: its total last tick, the turn not yet spent and
    /// the ticks until it can select again.
    wheel: Option<f32>,
    wheel_turn: f32,
    wheel_wait: u32,
}

/// A second finger held this long (ticks) is the slow move; lifted sooner,
/// having stayed put, it is a tap: the weapon select.
const SECOND_HOLD: u32 = 8;
/// After the wheel selects the weapon, it rests this many ticks (a
/// trackpad's scroll goes on long after the fingers lift).
const WHEEL_WAIT: u32 = 15;

/// In play, the triggers that touch and the mouse have no button for. A
/// second finger, anywhere, held: the slow move (while held); tapped: the
/// weapon select. A turn of the wheel either way: the weapon select. The
/// first finger steers and shoots as before.
fn play_extras(a: &ActionState, steer: &mut Steer) -> u16 {
    let total = a.raw.mouse.wheel;
    let turned = steer.wheel.map_or(0.0, |w| (total - w).abs());
    steer.wheel = Some(total);
    if steer.ship.is_none() {
        (steer.second, steer.wheel_turn, steer.wheel_wait) = (None, 0.0, 0);
        return 0;
    }
    let mut b = 0;
    if steer.wheel_wait > 0 {
        steer.wheel_wait -= 1;
        steer.wheel_turn = 0.0;
    } else {
        steer.wheel_turn += turned;
        if steer.wheel_turn >= 1.0 {
            b |= WEAPON_SELECT;
            (steer.wheel_turn, steer.wheel_wait) = (0.0, WHEEL_WAIT);
        }
    }
    let other = a.raw.fingers.iter().zip(&a.touches).find(|(f, _)| a.touching && Some(f.id) != a.primary).map(|(f, &p)| (f.id, p));
    match (steer.second, other) {
        (Some((id, ticks, travel, last)), Some((now, p))) if now == id => {
            let moved = (p[0] - last[0]).abs() + (p[1] - last[1]).abs();
            steer.second = Some((id, ticks + 1, travel + moved, p));
            if ticks + 1 >= SECOND_HOLD {
                b |= SLOW;
            }
        }
        (last, now) => {
            // Lifted soon and still (not just become the steering finger).
            if let Some((id, ticks, travel, _)) = last
                && ticks < SECOND_HOLD
                && travel < 12.0
                && a.primary != Some(id)
            {
                b |= WEAPON_SELECT;
            }
            steer.second = now.map(|(id, p)| (id, 1, 0.0, p));
        }
    }
    b
}

/// A swipe this long (arena units) is one menu step.
const SWIPE_STEP: f32 = 48.0;

/// Touch where there is no ship (the title's menu, name entry): a vertical
/// swipe is a step up or down, and a short tap that stays put is the shot
/// (confirm), on release. A finger held down is not the shot here, so a
/// swipe never confirms. The options screen reads its taps itself.
fn menu_touch(a: &ActionState, steer: &mut Steer, mut b: u16) -> u16 {
    if steer.ship.is_some() {
        (steer.swipe, steer.travel, steer.touch_ticks, steer.was_touching) = ([0.0; 2], 0.0, 0, false);
        return b;
    }
    let fingers = !a.raw.fingers.is_empty();
    if fingers {
        b &= !SHOT;
        steer.swipe[0] += a.drag[0];
        steer.swipe[1] += a.drag[1];
        steer.travel += a.drag[0].abs() + a.drag[1].abs();
        steer.touch_ticks += 1;
        let [x, y] = steer.swipe;
        if y.abs() >= SWIPE_STEP && y.abs() > x.abs() {
            b |= if y < 0.0 { UP } else { DOWN };
            steer.swipe = [0.0; 2];
        }
    } else if steer.was_touching {
        if steer.touch_ticks < 30 && steer.travel < 12.0 {
            b |= SHOT;
        }
        (steer.swipe, steer.travel, steer.touch_ticks) = ([0.0; 2], 0.0, 0);
    }
    steer.was_touching = fingers;
    b
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
        b |= play_extras(a, steer);
        let b = menu_touch(a, steer, b);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Finger;

    fn touch(at: Option<[f32; 2]>, drag: [f32; 2]) -> ActionState {
        let mut a = ActionState::default();
        (a.drag, a.touching) = (drag, at.is_some());
        if let Some(pos) = at {
            a.raw.fingers.push(Finger { id: 1, pos });
        }
        a
    }

    /// Fingers (id, place) down this tick, the first one steering.
    fn fingers(f: &[(u64, [f32; 2])]) -> ActionState {
        let mut a = ActionState::default();
        a.touching = !f.is_empty();
        a.primary = f.first().map(|f| f.0);
        for &(id, pos) in f {
            a.raw.fingers.push(Finger { id, pos: [0.5; 2] });
            a.touches.push(pos);
        }
        a
    }

    #[test]
    fn second_finger_holds_slow_and_taps_weapon() {
        let mut steer = Steer { ship: Some([320.0, 400.0]), ..Default::default() };
        let one = fingers(&[(1, [100.0, 100.0])]);
        let two = fingers(&[(1, [100.0, 100.0]), (2, [300.0, 200.0])]);
        assert_eq!(play_extras(&one, &mut steer), 0);
        // A quick tap: nothing while down, the weapon select on release.
        for _ in 0..3 {
            assert_eq!(play_extras(&two, &mut steer), 0);
        }
        assert_eq!(play_extras(&one, &mut steer), WEAPON_SELECT);
        assert_eq!(play_extras(&one, &mut steer), 0);
        // Held: the slow move from SECOND_HOLD ticks on, no select on release.
        let held: Vec<u16> = (0..SECOND_HOLD + 2).map(|_| play_extras(&two, &mut steer)).collect();
        assert_eq!(held[..SECOND_HOLD as usize - 1], vec![0; SECOND_HOLD as usize - 1][..]);
        assert!(held[SECOND_HOLD as usize..].iter().all(|&b| b == SLOW));
        assert_eq!(play_extras(&one, &mut steer), 0);
        // A tap that slides is not a select.
        play_extras(&two, &mut steer);
        play_extras(&fingers(&[(1, [100.0, 100.0]), (2, [340.0, 200.0])]), &mut steer);
        assert_eq!(play_extras(&one, &mut steer), 0);
        // The steering finger lifts and the other takes over: not a select.
        play_extras(&two, &mut steer);
        assert_eq!(play_extras(&fingers(&[(2, [300.0, 200.0])]), &mut steer), 0);
    }

    #[test]
    fn wheel_selects_the_weapon_once_per_turn() {
        let mut steer = Steer { ship: Some([320.0, 400.0]), ..Default::default() };
        let wheel = |w: f32| {
            let mut a = ActionState::default();
            a.raw.mouse.wheel = w;
            a
        };
        assert_eq!(play_extras(&wheel(5.0), &mut steer), 0);
        assert_eq!(play_extras(&wheel(5.5), &mut steer), 0);
        assert_eq!(play_extras(&wheel(4.5), &mut steer), WEAPON_SELECT);
        // A trackpad's long scroll: one select, then a rest.
        let after: Vec<u16> = (0..WHEEL_WAIT).map(|i| play_extras(&wheel(5.0 + i as f32), &mut steer)).collect();
        assert!(after.iter().all(|&b| b == 0));
        assert_eq!(play_extras(&wheel(30.0), &mut steer), WEAPON_SELECT);
        // No ship (the menus have their own wheel): nothing.
        steer.ship = None;
        assert_eq!(play_extras(&wheel(40.0), &mut steer), 0);
    }

    #[test]
    fn menu_tap_confirms_on_release_and_swipe_steps() {
        let mut steer = Steer::default();
        // A finger held down is not the shot in a menu.
        assert_eq!(menu_touch(&touch(Some([0.5; 2]), [0.0; 2]), &mut steer, SHOT), 0);
        // Released soon and still: the shot, once.
        assert_eq!(menu_touch(&touch(None, [0.0; 2]), &mut steer, 0), SHOT);
        assert_eq!(menu_touch(&touch(None, [0.0; 2]), &mut steer, 0), 0);
        // A swipe down: one step, then no shot on release.
        assert_eq!(menu_touch(&touch(Some([0.5; 2]), [0.0, 30.0]), &mut steer, 0), 0);
        assert_eq!(menu_touch(&touch(Some([0.5; 2]), [0.0, 30.0]), &mut steer, 0), DOWN);
        assert_eq!(menu_touch(&touch(None, [0.0; 2]), &mut steer, 0), 0);
        // With a ship, touch is play: nothing changes.
        steer.ship = Some([320.0, 400.0]);
        assert_eq!(menu_touch(&touch(Some([0.5; 2]), [0.0, 99.0]), &mut steer, SHOT), SHOT);
    }
}
