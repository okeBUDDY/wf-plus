//! The player (control 1, player.cpp) and its laser (LaserSprite,
//! line_sprites.cpp).

use super::controls::{LaserSprite, ids};
use super::{ActorDesc, AnmData, Aid, EffectDesc, Obj, Param, create, sheet, spawn};
use crate::game::Cx;
use crate::game::input::{DOWN, LEFT, RIGHT, SHOT, SLOW, UP, WEAPON_SELECT};
use crate::game::math::{cos, ftol, ftos, sin};
use crate::game::sounds;
use bytemuck::{Pod, Zeroable};

/// Actor::work of the player.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PlayerWork {
    flame: u32,
    laser: u32,
    weapon: i16,
    shots: i16,
    switch_time: i16,
    turn_time: i16,
    invincible: i16,
    _pad: i16,
}

/// g_flame_anim (0x45d4a4): add0 8, 9 looping.
static FLAME_ANIM: AnmData = AnmData { pat_file: sheet::ADD, script: &[8, 2, 9, 2, 8, 1, 9, 1, -2, 0, -1] };
/// g_weapon_sounds (0x45d4e0).
const WEAPON_SOUNDS: [usize; 2] = [13, 14];
/// g_player_bounds (0x45d4e8).
/// g_direction_angles (0x45d4f8) by the direction bits.
const DIRECTION_ANGLES: [i16; 16] = [-1, 0, 512, -1, 768, 896, 640, -1, 256, 128, 384, -1, -1, -1, -1, -1];

const LASER_MAX: f32 = 140.0;

impl LaserSprite {
    /// LaserSprite::Reset (0x415220).
    pub fn reset(&mut self, x: f32, y: f32) {
        self.tip_x = x;
        self.tip_y = y - LASER_MAX;
        self.x = x;
        self.y = y;
        self.angle = 0;
        self.spread = 16;
    }

    /// LaserSprite::Move (0x415250).
    pub fn move_to(&mut self, x: f32, y: f32, hold: bool, push_x: f32, push_y: f32) {
        if hold {
            self.tip_x = x + self.tip_x - self.x;
            self.tip_y = y + self.tip_y - self.y;
        } else {
            self.tip_x += push_x;
            self.tip_y += push_y;
            let dx = self.tip_x - x;
            let dy = self.tip_y - y;
            let mut length = ((dy * dy + dx * dx) as f64).sqrt() as f32;
            let angle = libm::atan2(dx as f64, -dy as f64) as f32;
            if length > LASER_MAX {
                length = LASER_MAX;
                self.tip_x = (libm::sin(angle as f64) as f32) * LASER_MAX + x;
                self.tip_y = y - (libm::cos(angle as f64) as f32) * LASER_MAX;
            }
            self.angle = ftos(angle * 162.974_66);
            self.spread = 160i16.wrapping_sub(ftos(length * 1.028_571_5));
        }
        let left = self.angle.wrapping_add(self.spread) as i32;
        let right = self.angle.wrapping_sub(self.spread) as i32;
        self.points[0] = [sin(left) * 2048.0, cos(left) * -2048.0];
        self.points[2] = [sin(right) * 2048.0, cos(right) * -2048.0];
        self.x = x;
        self.y = y;
    }
}

/// PlayerControl::SelectWeapon: `weapon` (the next one when negative).
fn select_weapon(cx: &mut Cx, a: Aid, mut weapon: i32, x: f32, y: f32) {
    let w = *cx.actors.w::<PlayerWork>(a);
    if weapon < 0 {
        weapon = w.weapon as i32 + 1;
    }
    if weapon >= 2 {
        weapon = 0;
    }
    let pw = cx.actors.w::<PlayerWork>(a);
    pw.weapon = weapon as i16;
    pw.shots = 0;
    cx.actors.get_mut(w.flame as Aid).angle = 0;
    cx.ctl.weapon = weapon;
    cx.actors.get_mut(w.laser as Aid).color = 0;
    if weapon == 1 {
        cx.actors.get_mut(w.laser as Aid).color = 0xaa00_ff00;
        cx.ctl.laser.reset(x, y);
    }
}

/// PlayerControl::OnCreate (0x413550).
pub fn on_create(cx: &mut Cx, a: Aid) -> bool {
    {
        let x = cx.actors.get_mut(a);
        x.hit_points = 1;
        x.team = 0x60;
        x.debris = ids::PLAYER_EXPLOSION;
    }
    cx.actors.set_layer(a, 2);
    let sprite = cx.pats.get(sheet::SYS, 1);
    cx.actors.set_sprite(a, sprite);
    let flame_sprite = cx.actors.play(&FLAME_ANIM);
    let desc = ActorDesc { color: 0xffff_ffff, flags: super::flags::ADDITIVE, hit_points: 1, sprite: flame_sprite, parent: Some(a), layer: 11, ..Default::default() };
    let flame = spawn(cx, &desc).unwrap_or(0);
    cx.actors.get_mut(flame).color = 0;
    let desc = ActorDesc { color: 0xffff_ffff, hit_points: 1, sprite: Obj::Laser, parent: Some(a), layer: 1, ..Default::default() };
    let laser = spawn(cx, &desc).unwrap_or(0);
    let w = cx.actors.w::<PlayerWork>(a);
    w.flame = flame as u32;
    w.laser = laser as u32;
    w.turn_time = 0;
    w.shots = 0;
    w.switch_time = 0;
    let x = cx.actors.get_mut(a);
    x.start_angle = 0;
    x.speed = 0.0;
    let (px, py) = (x.x(), x.y());
    let weapon = cx.ctl.weapon;
    select_weapon(cx, a, weapon, px, py);
    cx.actors.w::<PlayerWork>(a).invincible = 120;
    let x = cx.actors.get_mut(a);
    x.color = 0xffff_ffff;
    x.team = 0;
    true
}

/// PlayerControl::Events: invincible for `value` frames (for good when
/// negative, no longer when 0).
pub const EVENT_INVINCIBLE: i32 = 1;

/// PlayerControl::OnEvent (0x4130f0): EVENT_INVINCIBLE makes the player
/// invincible for `value` frames (for good when negative).
pub fn on_event(cx: &mut Cx, a: Aid, event: i32, value: i32) {
    if event != EVENT_INVINCIBLE {
        return;
    }
    cx.actors.w::<PlayerWork>(a).invincible = value as i16;
    if value != 0 {
        cx.actors.get_mut(a).team = 0;
        if value <= 0 {
            cx.actors.w::<PlayerWork>(a).invincible = -1;
        }
    } else {
        cx.actors.get_mut(a).team = 0x60;
    }
    cx.actors.get_mut(a).color = 0xffff_ffff;
}

/// PlayerControl::Fire (0x413130).
fn fire(cx: &mut Cx, a: Aid) {
    let w = *cx.actors.w::<PlayerWork>(a);
    let mut angle: i16 = 0;
    let (px, py) = (cx.actors.get(a).x(), cx.actors.get(a).y());
    match w.weapon {
        1 => {
            let mut spread = cx.ctl.laser.spread as i32;
            if w.turn_time != 0 {
                spread >>= 1;
            }
            angle = cx.ctl.laser.angle;
            let dx = sin(angle as i32) * 4.0;
            let dy = cos(angle as i32) * -4.0;
            cx.ctl.shot.x = dx + px;
            cx.ctl.shot.y = dy + py;
            cx.ctl.shot.start_angle = (cx.rng.rand() % spread + angle as i32) as u16;
            let d = cx.ctl.shot;
            spawn(cx, &d);
            cx.ctl.shot.x = dx + px;
            cx.ctl.shot.y = dy + py;
            cx.ctl.shot.start_angle = (angle as i32 - cx.rng.rand() % spread) as u16;
            let d = cx.ctl.shot;
            spawn(cx, &d);
            let flame = cx.actors.get_mut(w.flame as Aid);
            if flame.angle != angle {
                flame.angle = angle;
            }
        }
        0 => {
            let offset = (cx.rng.rand() & 7) as f32;
            let (mask, center) = if w.turn_time != 0 { (0xf, 7) } else { (0x3f, 0x1f) };
            let xs: &[Option<f32>] = if w.turn_time != 0 { &[Some(-offset), Some(offset), None] } else { &[Some(-offset), Some(offset)] };
            for &ox in xs {
                cx.ctl.shot.x = match ox {
                    Some(ox) => ox + px,
                    None => px,
                };
                cx.ctl.shot.y = py - 4.0;
                cx.ctl.shot.start_angle = ((cx.rng.rand() & mask) - center) as u16;
                let d = cx.ctl.shot;
                spawn(cx, &d);
            }
            if w.turn_time == 0 {
                cx.rng.rand();
            }
        }
        _ => {}
    }
    cx.ctl.muzzle.angle_min = (angle as i32 + 0x138) as i16;
    cx.ctl.muzzle.angle_max = (angle as i32 + 0x2c8) as i16;
    let e: EffectDesc = cx.ctl.muzzle;
    create(cx, ids::EFFECT.into(), None, px, py, 3, 0, Param::Effect(e));
    if w.shots % 3 == 0 {
        cx.play_sound(sounds::FIRE_ME);
    }
}

/// PlayerControl::OnMove (0x4136f0).
pub fn on_move(cx: &mut Cx, a: Aid) -> bool {
    let pressed = cx.fn_game.pressed;
    let keys = cx.fn_game.keys;
    let mut push_x = 0.0f32;
    let mut push_y = 0.0f32;
    let direction = DIRECTION_ANGLES[(keys & (UP | DOWN | LEFT | RIGHT)) as usize];
    let speed = if direction < 0 {
        0.0
    } else if keys & SLOW != 0 {
        2.0
    } else {
        7.0
    };
    // The bounds: 16, 32, 624, 448 in Classic (see `Arena::player_bounds`).
    #[allow(non_snake_case)]
    let BOUNDS = cx.arena.player_bounds();
    let (tx, ty) = (cx.actors.get(a).x(), cx.actors.get(a).y());
    let mut x = sin(direction as i32) * speed + tx;
    let mut y = ty - cos(direction as i32) * speed;
    if x < BOUNDS[0] {
        push_x = BOUNDS[0] - x;
        x = BOUNDS[0];
    } else if x > BOUNDS[2] {
        push_x = BOUNDS[2] - x;
        x = BOUNDS[2];
    }
    if y < BOUNDS[1] {
        push_y = BOUNDS[1] - y;
        y = BOUNDS[1];
    } else if y > BOUNDS[3] {
        push_y = BOUNDS[3] - y;
        y = BOUNDS[3];
    }
    cx.actors.get_mut(a).set_position(x, y);
    {
        let w = cx.actors.w::<PlayerWork>(a);
        if pressed & (UP | DOWN | LEFT | RIGHT) != 0 {
            w.turn_time = 8;
        } else if w.turn_time > 0 {
            w.turn_time -= 1;
        }
    }
    if pressed & WEAPON_SELECT != 0 {
        select_weapon(cx, a, -1, x, y);
        cx.play_sound(sounds::SWITCH);
        cx.actors.w::<PlayerWork>(a).switch_time = 30;
    }
    let firing = keys & SHOT != 0;
    cx.ctl.laser.move_to(x, y, firing, push_x, push_y);
    let w = *cx.actors.w::<PlayerWork>(a);
    if w.invincible != 0 {
        if w.invincible > 0 {
            let blink = if cx.app.frame_count & 2 != 0 { 0xffff_ffff } else { 0 };
            cx.actors.get_mut(a).color = blink;
            let pw = cx.actors.w::<PlayerWork>(a);
            pw.invincible -= 1;
            if pw.invincible == 0 {
                let x = cx.actors.get_mut(a);
                x.team = 0x60;
                x.color = 0xffff_ffff;
            }
        }
    } else if cx.actors.flags & 1 != 0
        && let Some(hit) = cx.actors.hit_test(a) {
            crate::game::enm::on_actor_destroyed(cx, hit, a);
            return false;
        }
    if firing {
        fire(cx, a);
        cx.actors.w::<PlayerWork>(a).shots += 1;
        cx.actors.get_mut(w.flame as Aid).color = 0xffff_ffff;
    } else {
        cx.actors.get_mut(w.flame as Aid).color = 0;
        cx.actors.w::<PlayerWork>(a).shots = 0;
    }
    let pw = cx.actors.w::<PlayerWork>(a);
    if pw.switch_time > 0 {
        pw.switch_time -= 1;
        if pw.switch_time == 0 {
            let sound = WEAPON_SOUNDS[(pw.weapon as usize) & 1];
            cx.play_sound(sound);
        }
    }
    true
}

/// The player's state in a replay (PlayerRecord, PlayerControl::Save 0x413070).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct PlayerRecord {
    pub x: f32,
    pub y: f32,
    pub weapon: i32,
    pub tip_x: f32,
    pub tip_y: f32,
    pub angle: i16,
    pub spread: i16,
}

// PlayerControl::Save writes 24 bytes between the boss parts and the input.
const _: () = assert!(std::mem::size_of::<PlayerRecord>() == 24);

/// PlayerControl::Save.
pub fn save(cx: &Cx, a: Aid) -> PlayerRecord {
    let l = &cx.ctl.laser;
    let x = cx.actors.get(a);
    PlayerRecord { x: x.x(), y: x.y(), weapon: cx.ctl.weapon, tip_x: l.tip_x, tip_y: l.tip_y, angle: l.angle, spread: l.spread }
}

/// PlayerControl::Load (0x413440, matched).
pub fn load(cx: &mut Cx, a: Aid, r: &PlayerRecord) {
    cx.actors.get_mut(a).set_position(r.x, r.y);
    select_weapon(cx, a, r.weapon, r.x, r.y);
    let l = &mut cx.ctl.laser;
    l.tip_x = r.tip_x;
    l.tip_y = r.tip_y;
    l.x = r.x;
    l.y = r.y;
    l.angle = r.angle;
    l.spread = r.spread;
}

/// PlayerControl::ResetWeapon (0x412f90).
pub fn reset_weapon(cx: &mut Cx) {
    cx.ctl.weapon = 0;
}

#[allow(unused)]
fn _unused() -> i32 {
    ftol(0.0)
}
