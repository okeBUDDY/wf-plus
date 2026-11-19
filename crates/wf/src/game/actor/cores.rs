//! The weapons of the cores (controls 40..49) and what they fire: the
//! laser (43), the homing missile (45) with its smoke (21) and the slow
//! missile (48). core_controls.cpp.

use super::parts::{self, PartWork, init_core, move_part, on_damage, prepare, pw, schedule};
use super::{AnmData, Aid, Param, create, flags, sheet, spawn};
use crate::game::Cx;
use crate::game::math::{cos, ftos, sin, sin_table};
use crate::game::sounds;
use bytemuck::{Pod, Zeroable};

/// The tail of a core's work after PartWork (0x54..0x70).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CoreWork {
    p: PartWork,
    /// 0x54..0x6f, by control: see the accessors.
    t: [u8; 0x1c],
}

fn t16(cx: &mut Cx, a: Aid, at: usize) -> &mut i16 {
    let w = cx.actors.w::<CoreWork>(a);
    bytemuck::from_bytes_mut(&mut w.t[at..at + 2])
}
fn t32(cx: &mut Cx, a: Aid, at: usize) -> &mut i32 {
    let w = cx.actors.w::<CoreWork>(a);
    bytemuck::from_bytes_mut(&mut w.t[at..at + 4])
}
fn tf32(cx: &mut Cx, a: Aid, at: usize) -> &mut f32 {
    let w = cx.actors.w::<CoreWork>(a);
    bytemuck::from_bytes_mut(&mut w.t[at..at + 4])
}

// Offsets into the tail (from 0x54).
// Control40Work: interval 0x54, fire_angle 0x56, delay 0x58, speed 0x5c.
// Control41Work: interval 0x54, count 0x56, spread 0x58, speed 0x5c.
// Control42Work: laser 0x54, mode 0x58, duration 0x5a, delay 0x5c.
// Control44Work: interval 0x54, delay 0x56.
// Control46Work: count 0x54, base 0x56, range 0x58, delay 0x5a.
// Control47Work: speed 0x54, delay 0x58.
// Control49Work: start 0x54, sweep 0x58, delay 0x5a, amplitude 0x5c.

/// CorePartControl::Setup: the weapon's parameters from `rank`.
pub fn setup(cx: &mut Cx, id: i16, a: Aid, rank: i32) {
    match id {
        40 => {
            let interval = (rank / 10 * 4 + 4) as i16;
            *t16(cx, a, 0) = interval;
            *tf32(cx, a, 8) = 4.0;
            if rank > 50 {
                *tf32(cx, a, 8) = (rank - 50) as f32 * 0.0625 + 4.0;
            }
            *t16(cx, a, 4) = (50 - rank * 30 / 100) as i16;
            pw(cx, a).unknown_22 = interval;
        }
        41 => {
            let count = ((rank >> 3) + 1) as i16;
            *t16(cx, a, 2) = count;
            *t16(cx, a, 4) = 0x40;
            if count as i32 * 0x40 > 0x400 {
                *t16(cx, a, 4) = (0x400 / count as i32) as i16;
            }
            *tf32(cx, a, 8) = rank as f32 * 0.025 + 3.0;
            *t16(cx, a, 0) = (40 - rank * 20 / 100) as i16;
        }
        42 => {
            *t16(cx, a, 4) = if rank < 33 { 0 } else { (rank >= 66) as i16 + 1 };
            let duration = ((rank / 10 + 3) * 20) as i16;
            *t16(cx, a, 6) = duration;
            *t32(cx, a, 0) = 0;
            *t16(cx, a, 8) = (180 - rank * 90 / 100) as i16;
            pw(cx, a).unknown_22 = duration + 10;
        }
        44 => {
            let interval = (rank / 35 * 8 + 8) as i16;
            *t16(cx, a, 0) = interval;
            *t16(cx, a, 2) = (120 - rank * 60 / 100) as i16;
            pw(cx, a).unknown_22 = interval;
        }
        46 => {
            *t16(cx, a, 0) = (rank * 25 / 100 + 5) as i16;
            *t16(cx, a, 4) = ((rank >> 2) + 30) as i16;
            *t16(cx, a, 2) = 25;
            *t16(cx, a, 6) = (120 - rank * 60 / 100) as i16;
        }
        47 => {
            *tf32(cx, a, 0) = rank as f32 * 0.05 + 5.0;
            *t16(cx, a, 4) = (240 - rank * 150 / 100) as i16;
        }
        _ => {
            let sweep = (rank * 240 / 100 + 120) as i16;
            *t16(cx, a, 4) = sweep;
            *t16(cx, a, 6) = (180 - rank * 120 / 100) as i16;
            pw(cx, a).unknown_22 = sweep;
        }
    }
}

/// The OnCreate of the cores (0x413aa0, 0x413ea0, 0x4140d0, 0x414700,
/// 0x414c10, 0x413c60, 0x414e80).
pub fn core_create(cx: &mut Cx, id: i16, a: Aid, param: Param) -> bool {
    let Param::Slot(slot) = param else { return true };
    init_core(cx, id, a, slot);
    let pat = match id {
        40 => 48,
        41 => 49,
        42 => 50,
        44 => 51,
        46 => 52,
        47 => 53,
        _ => 54,
    };
    match id {
        44 => {
            pw(cx, a).flags &= !0x10000;
        }
        49 => {}
        _ => {}
    }
    let sprite = cx.pats.get(sheet::SYS, pat);
    cx.actors.set_sprite(a, sprite);
    cx.actors.get_mut(a).debris = 17;
    match id {
        44 => on_damage(cx, a, 0),
        49 => {
            let w = pw(cx, a);
            w.flags &= !0x10000;
            let amplitude = (w.high as i32 - w.low as i32) / 2;
            w.angle = (w.low as i32 + amplitude) as i16;
            let (turret, angle) = (w.turret as Aid, w.angle as u16);
            *t16(cx, a, 8) = amplitude as i16;
            let t = cx.actors.get_mut(turret);
            t.start_angle = angle;
            t.angle = angle;
            on_damage(cx, a, 0);
        }
        _ => on_damage(cx, a, 6),
    }
    true
}

/// The OnMove of the cores.
pub fn core_move(cx: &mut Cx, id: i16, a: Aid) -> bool {
    if pw(cx, a).state != 5 {
        return move_part(cx, a);
    }
    let (sx, sy) = (cx.actors.get(a).sx(), cx.actors.get(a).sy());
    let slot = pw(cx, a).slot;
    match id {
        // Control40::OnMove (0x413b00): rounds of bullets.
        40 => {
            if pw(cx, a).state_time == 0 {
                let angle = cx.actors.screen_angle(a);
                *t16(cx, a, 2) = angle as i16;
                let interval = *t16(cx, a, 0);
                let w = pw(cx, a);
                w.unknown_1e = interval;
                w.state_time = 1;
            }
            if pw(cx, a).state_time == 1 {
                if pw(cx, a).unknown_1e & 3 == 0 {
                    let fire_angle = *t16(cx, a, 2) as u16;
                    let speed = *tf32(cx, a, 8);
                    let d = &mut cx.ctl.desc40;
                    d.x = sx;
                    d.y = sy;
                    d.start_angle = fire_angle;
                    d.speed = speed;
                    d.work = slot;
                    let d = *d;
                    spawn(cx, &d);
                    cx.play_sound(sounds::FIRE_BL);
                }
                let w = pw(cx, a);
                w.unknown_1e -= 1;
                if w.unknown_1e <= 0 {
                    let delay = *t16(cx, a, 4) as i32;
                    schedule(cx, a, delay, 6);
                }
            }
        }
        // Control41::OnMove (0x413f00): a fan of shells.
        41 => {
            let speed = *tf32(cx, a, 8);
            let count = *t16(cx, a, 2) as i32;
            let spread = *t16(cx, a, 4) as i32;
            let interval = *t16(cx, a, 0) as i32;
            let d = &mut cx.ctl.desc41;
            d.x = sx;
            d.y = sy;
            d.speed = speed;
            d.work = slot;
            let mut angle = cx.actors.screen_angle(a) as i16 as i32 - (((count - 1) * spread) >> 1);
            for _ in 0..count {
                cx.ctl.desc41.start_angle = angle as u16;
                let d = cx.ctl.desc41;
                spawn(cx, &d);
                angle += spread;
            }
            cx.play_sound(sounds::FIRE_CN);
            schedule(cx, a, interval, 6);
        }
        // Control42::OnMove (0x414130): the laser.
        42 => {
            if pw(cx, a).state_time == 0 {
                let laser = create(cx, 43, Some(a), 0.0, -16.0, 6, 0, Param::Actor(a));
                *t32(cx, a, 0) = laser.map_or(0, |l| l as i32);
                let Some(_) = laser else {
                    on_damage(cx, a, 6);
                    return true;
                };
                let duration = *t16(cx, a, 6);
                let w = pw(cx, a);
                w.state_time += 1;
                w.unknown_1e = duration;
            }
            match pw(cx, a).state_time {
                1 => {
                    let w = pw(cx, a);
                    w.unknown_1e -= 1;
                    if w.unknown_1e <= 0 {
                        let laser = *t32(cx, a, 0) as Aid;
                        if cx.actors.get(laser).control {
                            let lid = cx.actors.get(laser).id;
                            super::controls::on_event(cx, lid, laser, 2, 0);
                        }
                        let w = pw(cx, a);
                        w.state_time += 1;
                        w.unknown_1e = 10;
                        *t32(cx, a, 0) = 0;
                    }
                }
                2 => {
                    let w = pw(cx, a);
                    w.unknown_1e -= 1;
                    if w.unknown_1e <= 0 {
                        let delay = *t16(cx, a, 8) as i32;
                        schedule(cx, a, delay, 6);
                    }
                }
                _ => {}
            }
        }
        // Control44::OnMove (0x414760): homing missiles.
        44 => {
            if pw(cx, a).state_time == 0 {
                let interval = *t16(cx, a, 0);
                let w = pw(cx, a);
                w.unknown_1e = interval;
                w.state_time += 1;
            }
            if pw(cx, a).state_time == 1 {
                if pw(cx, a).unknown_1e & 7 == 0 && cx.actors.count < 700 {
                    let angle = cx.actors.screen_angle(a);
                    create(cx, 45, None, sx, sy, 11, angle, Param::Actor(a));
                    cx.play_sound(sounds::FIRE_MS);
                }
                let w = pw(cx, a);
                w.unknown_1e -= 1;
                if w.unknown_1e <= 0 {
                    let delay = *t16(cx, a, 2) as i32;
                    schedule(cx, a, delay, 0);
                }
            }
        }
        // Control46::OnMove (0x414c70): a spray of sparks.
        46 => {
            if cx.actors.count < 700 {
                let count = *t16(cx, a, 0) as i32;
                let base = *t16(cx, a, 2) as i32;
                let range = *t16(cx, a, 4) as i32;
                for _ in 0..count {
                    cx.ctl.desc46.work = slot;
                    let x = cx.actors.get(a).sx();
                    let rx = cx.rng.rand() & 7;
                    cx.ctl.desc46.x = x + rx as f32 - 4.0;
                    let y = cx.actors.get(a).sy();
                    let ry = cx.rng.rand() & 7;
                    cx.ctl.desc46.y = y + ry as f32 - 4.0;
                    let sa = cx.actors.screen_angle(a) as i32;
                    cx.ctl.desc46.start_angle = (sa + (cx.rng.rand() & 0x1f) - 16) as u16;
                    cx.ctl.desc46.angle = (cx.rng.rand() & 0x3ff) as u16;
                    cx.ctl.desc46.speed = ((cx.rng.rand() % range + base) as f32) * 0.1;
                    let d = cx.ctl.desc46;
                    spawn(cx, &d);
                }
                cx.play_sound(sounds::FIRE_MS);
            }
            let delay = *t16(cx, a, 6) as i32;
            schedule(cx, a, delay, 6);
        }
        // Control47::OnMove (0x413cc0): slow missiles.
        47 => {
            let angle = cx.actors.screen_angle(a);
            create(cx, 48, None, sx, sy, 11, angle, Param::Actor(a));
            cx.play_sound(sounds::FIRE_MS);
            let delay = *t16(cx, a, 4) as i32;
            schedule(cx, a, delay, 6);
        }
        // Control49::OnMove (0x414f20): a sweeping stream.
        _ => {
            if pw(cx, a).state_time == 0 {
                let age = cx.actors.get(a).age as i32;
                *t32(cx, a, 0) = age;
                let sweep = *t16(cx, a, 4);
                let w = pw(cx, a);
                w.unknown_1e = sweep;
                w.state_time += 1;
            }
            if pw(cx, a).state_time == 1 {
                let start = *t32(cx, a, 0);
                let amplitude = *t16(cx, a, 8) as f32;
                let age = cx.actors.get(a).age as i32;
                let w = *pw(cx, a);
                let v = ftos(amplitude * sin_table()[(((age - start) * 2) & 0x3ff) as usize]);
                cx.actors.get_mut(w.turret as Aid).angle = (v as i32 + w.angle as i32) as u16;
                if w.unknown_1e & 7 == 0 && cx.actors.count < 700 {
                    let angle = cx.actors.screen_angle(a);
                    let d = &mut cx.ctl.desc49;
                    d.x = sx;
                    d.y = sy;
                    d.start_angle = angle;
                    d.work = slot;
                    let d = *d;
                    spawn(cx, &d);
                    cx.play_sound(sounds::FIRE_BL);
                }
                let w = pw(cx, a);
                w.unknown_1e -= 1;
                if w.unknown_1e <= 0 {
                    on_damage(cx, a, 3);
                    let delay = *t16(cx, a, 6);
                    pw(cx, a).unknown_1e = delay;
                }
            }
        }
    }
    true
}

/// Actor::work of control 43.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LaserWork {
    owner: u32,
    scale: f32,
    glow: u32,
    head: u32,
    timer: i16,
    sound: i16,
}

/// The animations of the beam by Control42Work::mode (0x45d8xx).
static BEAM_ANIMS: [AnmData; 3] = [
    AnmData { set: sheet::ADD, script: &[17, 40, 18, -1] },
    AnmData { set: sheet::ADD, script: &[17, 40, 19, -1] },
    AnmData { set: sheet::ADD, script: &[17, 40, 20, -1] },
];
/// The width of the beam by mode (g_beam_scales).
const BEAM_SCALES: [f32; 3] = [1.0, 1.5, 2.0];

/// Control43::OnCreate (0x414210): the beam of control 42.
pub fn laser_create(cx: &mut Cx, a: Aid, param: Param) -> bool {
    let Param::Actor(owner) = param else { return false };
    {
        let w = cx.actors.w::<LaserWork>(a);
        w.owner = owner as u32;
        w.sound = 0;
    }
    let act = cx.actors.get_mut(a);
    act.hit_points = 1;
    act.team = 0;
    let owner_slot = pw(cx, owner).slot;
    let mode = (*t16(cx, owner, 4)).clamp(0, 2) as usize;
    cx.ctl.beam_desc.work = owner_slot;
    cx.ctl.beam_desc.y = -24.0;
    cx.ctl.beam_desc.parent = Some(a);
    cx.ctl.beam_desc.sprite = cx.actors.play(&BEAM_ANIMS[mode]);
    let d = cx.ctl.beam_desc;
    let Some(head) = spawn(cx, &d) else {
        cx.actors.w::<LaserWork>(a).head = 0;
        return false;
    };
    cx.actors.w::<LaserWork>(a).head = head as u32;
    let angle = cx.actors.screen_angle(a) as i16 as i32;
    let dx = sin(angle) * 42.0;
    let dy = cos(angle) * -42.0;
    let parent = cx.actors.get(a).parent;
    let mut x = cx.actors.get(parent).sx();
    let mut y = cx.actors.get(parent).sy();
    cx.ctl.beam_desc.y = -42.0;
    cx.ctl.beam_desc.parent = Some(head);
    for i in 0..20 {
        cx.ctl.beam_desc.sprite = cx.actors.play(&BEAM_ANIMS[mode]);
        let d = cx.ctl.beam_desc;
        if spawn(cx, &d).is_none() {
            return false;
        }
        cx.ctl.beam_desc.y -= 42.0;
        x += dx;
        y += dy;
        if i > 8 && (!(-120.0..=760.0).contains(&x) || !(-120.0..=600.0).contains(&y)) {
            break;
        }
    }
    cx.ctl.glow_desc.parent = Some(a);
    let d = cx.ctl.glow_desc;
    let Some(glow) = spawn(cx, &d) else { return false };
    let scale = BEAM_SCALES[mode];
    let w = cx.actors.w::<LaserWork>(a);
    w.glow = glow as u32;
    w.scale = scale;
    cx.actors.get_mut(glow).set_scale(scale);
    cx.actors.w::<LaserWork>(a).timer = 0;
    cx.play_sound(sounds::FIRE_LS1);
    cx.ctl.sound_users[0] += 1;
    cx.actors.w::<LaserWork>(a).sound = 0;
    true
}

/// Control43::OnDestroy (0x414430).
pub fn laser_destroy(cx: &mut Cx, a: Aid) {
    let sound = cx.actors.w::<LaserWork>(a).sound;
    release_sound(cx, sound);
}

fn release_sound(cx: &mut Cx, sound: i16) {
    if let 0 | 1 = sound {
        let i = sound as usize;
        cx.ctl.sound_users[i] -= 1;
        if cx.ctl.sound_users[i] == 0 {
            cx.sound.stop_channel(19 + i);
        }
    }
}

/// Control43::OnMove (0x414480).
pub fn laser_move(cx: &mut Cx, a: Aid) -> bool {
    let w = *cx.actors.w::<LaserWork>(a);
    let offset = -6 - (cx.rng.rand() & 0x1f);
    let head = cx.actors.get_mut(w.head as Aid);
    head.tf_mut().m[2][1] = offset as f32;
    head.flags |= flags::MOVED;
    let r = cx.rng.rand() & 0x1f;
    cx.actors.get_mut(w.glow as Aid).set_scale((r + 64) as f32 * w.scale / 64.0);
    if cx.actors.get(a).age == 40 {
        cx.ctl.sound_users[0] -= 1;
        if cx.ctl.sound_users[0] == 0 {
            cx.sound.stop_channel(19);
        }
        cx.play_sound(sounds::FIRE_LS2);
        cx.ctl.sound_users[1] += 1;
        cx.actors.w::<LaserWork>(a).sound = 1;
    }
    let w = cx.actors.w::<LaserWork>(a);
    if w.timer != 0 {
        w.timer -= 1;
        if w.timer <= 0 {
            return false;
        }
    }
    true
}

/// Control43::OnEvent (0x414550): event 2 ends the beam.
pub fn laser_event(cx: &mut Cx, a: Aid, x: i32) {
    let w = *cx.actors.w::<LaserWork>(a);
    if x != 2 || w.sound == 2 {
        return;
    }
    let fade = cx.pats.get(sheet::ADD, 17);
    let mut seg = w.head as Aid;
    cx.actors.set_sprite(seg, fade);
    seg = cx.actors.get(seg).first_child;
    while seg != super::NIL {
        cx.actors.set_sprite(seg, fade);
        seg = cx.actors.get(seg).prev_sibling;
    }
    cx.actors.w::<LaserWork>(a).scale = 1.0;
    release_sound(cx, w.sound);
    cx.play_sound(sounds::FIRE_LS3);
    let w = cx.actors.w::<LaserWork>(a);
    w.timer = 10;
    w.sound = 2;
}

/// Actor::work of control 45.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MissileWork {
    slot: u32,
    flags: u32,
    launcher: u32,
    homing: i16,
    _pad: i16,
}

/// Control45::OnCreate (0x414830): a homing missile of control 44.
pub fn missile_create(cx: &mut Cx, a: Aid, param: Param) -> bool {
    let Param::Actor(launcher) = param else { return false };
    let lw = *pw(cx, launcher);
    // Prepare() takes the launcher's work as the slot: its first field.
    prepare(cx, 45, a, lw.slot as u16);
    let sprite = cx.pats.get(sheet::SYS, 72);
    cx.actors.set_sprite(a, sprite);
    let act = cx.actors.get_mut(a);
    act.team = 0x61;
    act.debris = 17;
    act.speed = lw.rank as f32 * 0.014_285_714 + 4.0;
    act.angle = act.start_angle;
    let w = cx.actors.w::<MissileWork>(a);
    w.launcher = launcher as u32;
    w.slot = lw.slot;
    w.homing = (lw.rank as i32 * 180 / 100 + 40) as i16;
    true
}

/// Control45::OnMove (0x414ac0).
pub fn missile_move(cx: &mut Cx, a: Aid) -> bool {
    let age = cx.actors.get(a).age;
    if age > 30 && cx.actors.w::<MissileWork>(a).homing > 0 {
        let player = cx.game.player;
        cx.actors.turn_to(a, player, 8);
        let act = cx.actors.get_mut(a);
        act.start_angle = act.angle;
        cx.actors.w::<MissileWork>(a).homing -= 1;
    }
    if parts::is_off_screen(cx, a) {
        cx.actors.w::<MissileWork>(a).homing = 0;
    }
    if age & 1 != 0 {
        let (x, y) = (cx.actors.get(a).sx(), cx.actors.get(a).sy());
        create(cx, 21, None, x, y, 12, 0, Param::None);
    }
    true
}

/// Control48::OnCreate (0x413d50): a slow missile of control 47.
pub fn slow_missile_create(cx: &mut Cx, a: Aid, param: Param) -> bool {
    let Param::Actor(launcher) = param else { return false };
    let speed = *tf32(cx, launcher, 0);
    cx.actors.w::<[u32; 3]>(a)[2] = launcher as u32;
    let sprite = cx.pats.get(sheet::SYS, 67);
    cx.actors.set_sprite(a, sprite);
    let act = cx.actors.get_mut(a);
    act.team = 0x61;
    act.debris = 19;
    act.angle = act.start_angle;
    act.acceleration = -0.05;
    act.speed = speed;
    true
}

/// Actor::work of control 21, the smoke of a missile.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SmokeWork {
    alpha: i16,
    _pad: i16,
    scale: f32,
}

/// The smoke's colours by age (g_smoke_colors, 0: keep).
const SMOKE_COLORS: [u32; 21] = [
    0xffff_ffff, 0, 0xfffe_fbb2, 0, 0xfffe_f97f, 0, 0, 0, 0xfffe_c15d, 0, 0, 0, 0xffff_8337, 0, 0, 0, 0xffc0_3d19, 0, 0, 0, 0,
];

/// Control21::OnCreate (0x4148d0).
pub fn smoke_create(cx: &mut Cx, a: Aid) -> bool {
    let sprite = cx.pats.get(sheet::ADD, 24);
    cx.actors.set_sprite(a, sprite);
    let act = cx.actors.get_mut(a);
    act.hit_points = 1;
    act.team = 0;
    act.flags |= flags::ADDITIVE;
    act.color = 0xffff_ffff;
    let w = cx.actors.w::<SmokeWork>(a);
    w.scale = 0.5;
    w.alpha = 0x80;
    cx.actors.get_mut(a).set_scale(0.5);
    true
}

/// Control21::OnMove (0x414940).
pub fn smoke_move(cx: &mut Cx, a: Aid) -> bool {
    let age = cx.actors.get(a).age;
    if age < 21 {
        let c = SMOKE_COLORS[age as usize];
        if c != 0 {
            cx.actors.get_mut(a).color = c;
        }
        cx.actors.w::<SmokeWork>(a).scale += 0.03;
    } else if age == 21 {
        let sprite = cx.pats.get(sheet::SYS, 73);
        cx.actors.set_sprite(a, sprite);
        let alpha = cx.actors.w::<SmokeWork>(a).alpha as u32;
        let act = cx.actors.get_mut(a);
        act.flags &= !flags::ADDITIVE;
        act.color = (alpha << 24) | 0xff_ffff;
        cx.actors.w::<SmokeWork>(a).scale += 0.2;
        let r = cx.rng.rand();
        cx.actors.get_mut(a).angle = (r & 0x3ff) as u16;
    } else {
        let w = cx.actors.w::<SmokeWork>(a);
        w.alpha -= 4;
        if w.alpha <= 0 {
            return false;
        }
        let alpha = w.alpha as u32;
        cx.actors.get_mut(a).color = (alpha << 24) | 0xff_ffff;
    }
    let scale = cx.actors.w::<SmokeWork>(a).scale;
    cx.actors.get_mut(a).set_scale(scale);
    true
}
