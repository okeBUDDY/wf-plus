//! The boss: its body (control 32) and its programs, the linked parts (33,
//! 34) and the turrets left on a stripped part (35). boss_controls.cpp,
//! boss_helpers.cpp.

use super::controls::send_event;
use super::parts::{self, PartPiece, init_linked, pw, scale_hit_points, spawn_children, spawn_items, spawn_pieces};
use super::{Aid, NIL, Param, create, flags, sheet, spawn};
use crate::game::Cx;
use crate::game::enm::{EnmList, child_base};
use crate::game::math::{cos, ftos, sin};
use crate::game::sounds;
use bytemuck::{Pod, Zeroable};

/// Actor::work of the body (Control32Work).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BodyWork {
    /// The first 0x20 bytes of PartWork.
    part: [u32; 8],
    /// The parts on the left (0, 1) and right (2, 3).
    arms: [u32; 4],
    deceleration: f32,
    brake_time: i16,
    turn_from: u16,
    turn: i16,
    turn_peak: i16,
    turn_end: i16,
    _pad: i16,
}

const DASHING: u32 = 0x1;
const TURNING: u32 = 0x2;

fn bw(cx: &mut Cx, a: Aid) -> &mut BodyWork {
    cx.actors.w::<BodyWork>(a)
}

/// g_body_pieces (0x45d418).
const BODY_PIECES: [PartPiece; 4] = [
    PartPiece { x: 0.0, y: 0.0, layer: 0, debris: 2, pat: 32, sheet: sheet::SYS },
    PartPiece { x: 0.0, y: -32.0, layer: 1, debris: 0, pat: 65, sheet: sheet::ENM },
    PartPiece { x: 0.0, y: 32.0, layer: 1, debris: 0, pat: 65, sheet: sheet::ENM },
    PartPiece { x: 0.0, y: 56.0, layer: 1, debris: 0, pat: 65, sheet: sheet::ENM },
];

fn mirror(cx: &mut Cx, a: Option<Aid>, flag: u32) {
    if let Some(a) = a {
        let x = cx.actors.get_mut(a);
        x.flags |= flag;
        x.matrix_angle = x.matrix_angle.wrapping_add(0x1000);
    }
}

/// Control32::OnCreate (0x4116f0).
pub fn body_create(cx: &mut Cx, a: Aid, param: Param) -> bool {
    let Param::Slot(slot) = param else { return false };
    cx.ctl.boss = Some(a);
    init_linked(cx, 32, a, slot);
    let hp = cx.enm.state.unknown_24 as i32;
    let sprite = cx.pats.get(sheet::ENM, 1);
    cx.actors.set_sprite(a, sprite);
    {
        let x = cx.actors.get_mut(a);
        x.hit_points = hp;
        x.team = 0x21;
        x.debris = 20;
        x.bounded = true;
    }
    pw(cx, a).max_hit_points = hp;
    let mut arms = [None; 4];
    if let Some(s) = cx.enm.find_slot(0x4000_0000) {
        let p = *cx.enm.part(s);
        arms[0] = create(cx, 33, Some(a), p.item.x as f32, p.item.y as f32, 4, 0, Param::Slot(s));
        arms[1] = create(cx, 33, Some(a), (-1 - p.item.x as i32) as f32, p.item.y as f32, 4, 0, Param::Slot(s));
        mirror(cx, arms[1], flags::MIRROR_X);
    }
    if let Some(s) = cx.enm.find_slot(0x8000_0000) {
        let p = *cx.enm.part(s);
        arms[2] = create(cx, 33, Some(a), p.x1 as f32, p.y1 as f32, 4, 0, Param::Slot(s));
        mirror(cx, arms[2], flags::MIRROR_Y);
        arms[3] = create(cx, 33, Some(a), (-1 - p.x1 as i32) as f32, p.y1 as f32, 4, 0, Param::Slot(s));
        mirror(cx, arms[3], flags::MIRROR_X);
        mirror(cx, arms[3], flags::MIRROR_Y);
    }
    for (i, arm) in arms.iter().enumerate() {
        bw(cx, a).arms[i] = arm.map_or(0, |x| x as u32);
        if let Some(arm) = arm {
            let l = pw(cx, *arm).level;
            pw(cx, a).level += l;
        }
    }
    scale_hit_points(cx, a);
    spawn_pieces(cx, a, &BODY_PIECES);
    cx.ctl.boss_bounds.top = cx.actors.get(a).y();
    cx.enm.list_9c = EnmList { op: 0x10001, arg: 0, step: 0, time: 0 };
    true
}

/// Control34::OnCreate (0x412b90): a part with children and items.
pub fn c34_create(cx: &mut Cx, a: Aid, param: Param) -> bool {
    let Param::Slot(slot) = param else { return false };
    init_linked(cx, 34, a, slot);
    let s = cx.enm.slots[slot as usize];
    let p = *cx.enm.part(slot);
    let base = child_base(s.code);
    let count = p.extra_child_count as i32 + p.child_count as i32;
    spawn_children(cx, a, base, 0, count);
    if s.flags & 0x3000 != 0 {
        spawn_children(cx, a, base, count as u32, p.option_count as i32);
    }
    if s.flags & 0x4000 != 0 {
        spawn_children(cx, a, base, (p.option_count as i32 + count) as u32, p.option4_count as i32);
    }
    scale_hit_points(cx, a);
    spawn_items(cx, a);
    true
}

/// Control33::OnCreate (0x412c30): an arm of the body.
pub fn c33_create(cx: &mut Cx, a: Aid, param: Param) -> bool {
    let Param::Slot(slot) = param else { return false };
    init_linked(cx, 33, a, slot);
    let base = child_base(cx.enm.slots[slot as usize].code);
    spawn_children(cx, a, base, 1, 2);
    scale_hit_points(cx, a);
    true
}

/// Control32::OnEvent (0x411d10).
pub fn body_event(cx: &mut Cx, a: Aid, x: i32, y: i32) {
    let arms = bw(cx, a).arms;
    if y & 0x400 == 0 {
        send_event(cx, arms[0] as Aid, x, y);
        send_event(cx, arms[1] as Aid, x, y);
    }
    if y & 0x200 == 0 {
        send_event(cx, arms[2] as Aid, x, y);
        send_event(cx, arms[3] as Aid, x, y);
    }
}

/// Control32::OnDestroy (0x411550).
pub fn body_destroy(cx: &mut Cx, a: Aid) {
    parts::part_destroy(cx, a);
    if cx.game.flags & crate::game::game_state::GAME_OVER == 0 {
        cx.fn_game.set_state(40);
    }
    cx.ctl.boss = None;
}

/// Control32::Dash (0x411e20): a dash toward (`x`, `y`), kept within the
/// bounds; false when it is there already.
fn dash(cx: &mut Cx, a: Aid, program: &mut EnmList, mut x: f32, mut y: f32, power: f32, boost: bool) -> bool {
    let b = cx.ctl.boss_bounds;
    let (from_x, from_y) = (cx.actors.get(a).x(), cx.actors.get(a).y());
    if x < b.left {
        y = (b.left - from_x) * (y - from_y) / (x - from_x) + from_y;
        x = b.left;
    } else if x > b.right {
        y = (b.right - from_x) * (y - from_y) / (x - from_x) + from_y;
        x = b.right;
    }
    if y < b.top {
        x = (b.top - from_y) * (x - from_x) / (y - from_y) + from_x;
        y = b.top;
    } else if y > b.bottom {
        x = (b.bottom - from_y) * (x - from_x) / (y - from_y) + from_x;
        y = b.bottom;
    }
    let dx = x - cx.actors.get(a).x();
    let dy = y - cx.actors.get(a).y();
    let distance2 = dy * dy + dx * dx;
    let acceleration = power * 0.005;
    bw(cx, a).deceleration = acceleration;
    if !(distance2 > 0.0) {
        return false;
    }
    let distance = (distance2 as f64).sqrt() as f32;
    let speed = ((distance * acceleration * 2.0 / ((acceleration + acceleration) * acceleration)) as f64).sqrt() as f32;
    let mut brake = ftos(speed / acceleration * acceleration);
    if brake <= 0 {
        brake = 1;
    }
    bw(cx, a).brake_time = brake;
    let angle = cx.actors.angle_to(a, x, y);
    let act = cx.actors.get_mut(a);
    act.acceleration = acceleration;
    act.speed = 0.0;
    act.start_angle = angle as u16;
    program.time = 0;
    pw(cx, a).flags |= DASHING;
    if boost {
        cx.enm.unknown_164 = 0;
        cx.enm.unknown_158 = angle as u16 as i32; // via the u16 start_angle: zero-extended
        cx.enm.flare_time = brake as i32;
        cx.enm.flare_size = power;
        let arms = bw(cx, a).arms;
        for arm in arms {
            send_event(cx, arm as Aid, 9, 0x110a);
        }
        if power > 1.0 && brake > 90 {
            cx.play_sound(sounds::BOOST2);
            return true;
        }
        cx.play_sound(sounds::BOOST1);
    }
    true
}

fn dash_power(p: &EnmList) -> f32 {
    if p.arg > 0 { p.arg as f32 * 0.01 } else { 1.0 }
}

/// Control32::RunProgram9c (0x4120d0): the movement program.
fn run_program_9c(cx: &mut Cx, a: Aid, p: &mut EnmList) {
    if pw(cx, a).flags & DASHING != 0 {
        let w = *bw(cx, a);
        if w.brake_time != 0 && p.time >= w.brake_time as i32 {
            cx.actors.get_mut(a).acceleration = -w.deceleration;
            bw(cx, a).brake_time = 0;
        }
        if !(cx.actors.get(a).speed > 0.0) {
            pw(cx, a).flags &= !DASHING;
            p.op = 0;
            p.time += 1;
            return;
        }
    } else {
        let ok = match p.op {
            0x10002 => {
                // The arguments go right to left: y's rand() first.
                let power = dash_power(p);
                let y = (cx.rng.rand() % 100 - 50) as f32 + 160.0;
                let x = (cx.rng.rand() % 320 - 160) as f32 + 320.0;
                dash(cx, a, p, x, y, power, false)
            }
            0x10005 => {
                let power = dash_power(p);
                let (x, y) = match cx.game.player {
                    Some(pl) => (cx.actors.get(pl).x(), cx.actors.get(pl).y()),
                    None => (320.0, 400.0),
                };
                dash(cx, a, p, x, y, power, true)
            }
            0x10004 => {
                let power = dash_power(p);
                dash(cx, a, p, 320.0, 160.0, power, false)
            }
            0x10003 => {
                let power = dash_power(p);
                let act = cx.actors.get(a);
                let angle = act.angle as i32;
                let x = sin(angle - 0x200) * 4096.0 + act.x();
                let y = act.y() - sin(angle - 0x100) * 4096.0;
                dash(cx, a, p, x, y, power * 6.0, true)
            }
            0x10006 => {
                let act = cx.actors.get_mut(a);
                act.start_angle = p.arg as u16;
                act.speed = 1.6;
                act.acceleration = -0.02;
                pw(cx, a).flags |= DASHING;
                bw(cx, a).brake_time = 0;
                true
            }
            0x10001 => {
                if p.step == 0 {
                    let act = cx.actors.get_mut(a);
                    act.start_angle = 0x200;
                    act.acceleration = 0.0;
                    act.speed = 5.0;
                    p.step += 1;
                }
                if p.step == 1 {
                    let act = cx.actors.get_mut(a);
                    if !(act.speed > 0.0) {
                        cx.enm.state.flags |= 0x100;
                        cx.ctl.boss_bounds.top = 64.0;
                        p.op = 0;
                        p.time += 1;
                        return;
                    }
                    if act.y() > 0.0 && act.acceleration == 0.0 {
                        act.acceleration = act.speed * -0.016_666_668;
                    }
                }
                true
            }
            _ => true,
        };
        if !ok {
            p.op = 0;
            p.time += 1;
            return;
        }
    }
    p.time += 1;
}

/// Control32::ListWeapons (0x411600).
fn list_weapons(cx: &mut Cx, side: usize) -> usize {
    let mut count = 0;
    for i in 0..7 {
        if cx.enm.core_power(side, i) != 0 {
            cx.ctl.weapons[count] = i as i32;
            count += 1;
        }
    }
    count
}

/// Control32::SignalSide (0x411960).
fn signal_side(cx: &mut Cx, a: Aid, side: usize, x: i32, y: i32) {
    let arms = bw(cx, a).arms;
    let (p, q) = if side != 0 { (arms[2], arms[3]) } else { (arms[0], arms[1]) };
    send_event(cx, p as Aid, x, y);
    send_event(cx, q as Aid, x, y);
}

/// Control32::AngleToPlayer (0x4124f0).
fn angle_to_player(cx: &mut Cx, a: Aid) -> u16 {
    match cx.game.player {
        Some(p) => {
            let (x, y) = (cx.actors.get(p).sx(), cx.actors.get(p).sy());
            cx.actors.angle_to(a, x, y) as u16
        }
        None => cx.actors.angle_to(a, 320.0, 400.0) as u16,
    }
}

/// The volley step shared by the side programs 0x30002..0x30007.
fn volley(cx: &mut Cx, a: Aid, p: &mut EnmList, side: usize, weapon: Option<usize>, y: i32, wait_peak: bool) {
    match p.step {
        0 => {
            if cx.enm.fire_wait[side] > 0 {
                return;
            }
            match weapon {
                Some(w) if cx.enm.core_power(side, w) <= 0 => {
                    p.op = 0;
                    return;
                }
                None if list_weapons(cx, side) == 0 => {
                    p.op = 0;
                    return;
                }
                _ => {}
            }
            cx.ctl.max_unknown_22 = 0;
            signal_side(cx, a, side, 5, y);
            let add = match weapon {
                Some(w) => cx.enm.core_power(side, w) * 10,
                None => (0..7).map(|i| cx.enm.core_power(side, i)).fold(0i32, |s, v| s.wrapping_add(v)) * 10,
            };
            cx.enm.fire_wait[side] += add;
            p.time = if wait_peak { cx.ctl.max_unknown_22 } else { 0 };
            p.step += 1;
        }
        1 => {
            p.time -= 1;
            if p.time < 0 && cx.enm.fire_wait[side] <= 0 {
                p.op = 0;
            }
        }
        _ => {}
    }
}

/// Control32::RunSideProgram (0x4119d0): the volleys of one side.
fn run_side_program(cx: &mut Cx, a: Aid, p: &mut EnmList, side: usize) {
    match p.op {
        0x30001 => {
            if cx.enm.fire_wait[side] > 0 {
                return;
            }
            let count = list_weapons(cx, side);
            if count == 0 {
                p.op = 0;
                return;
            }
            let weapon = cx.ctl.weapons[(cx.rng.rand() % count as i32) as usize];
            let x = if cx.rng.rand() & 3 != 0 { 5 } else { 7 };
            signal_side(cx, a, side, x, (0x10000 << weapon) | 0x10a);
            let mut add = cx.enm.core_power(side, weapon as usize) * 10;
            if p.arg > 0 {
                add = add * 100 / p.arg;
            }
            cx.enm.fire_wait[side] += add;
        }
        0x30002 | 0x30003 => {
            let w = p.arg as usize;
            volley(cx, a, p, side, Some(w), (0x10000 << p.arg) | 0x10a, p.op == 0x30003);
        }
        0x30004 | 0x30005 => {
            let w = p.arg as usize;
            volley(cx, a, p, side, Some(w), (0x10000 << p.arg) | 0x90a, p.op == 0x30005);
        }
        0x30006 | 0x30007 => {
            volley(cx, a, p, side, None, 0xffff_010au32 as i32, p.op == 0x30007);
        }
        _ => {}
    }
}

/// Control32::StartTurn (0x411580).
fn start_turn(cx: &mut Cx, a: Aid, p: &mut EnmList, angle: i16) -> bool {
    let from = cx.actors.get(a).angle;
    let w = bw(cx, a);
    w.turn_from = from as u16;
    w.turn = angle.wrapping_sub(from);
    let size = (w.turn as i32).abs() as i16;
    if size == 0 {
        return false;
    }
    if size > 90 {
        w.turn_peak = size;
        w.turn_end = size + 90;
    } else {
        w.turn_peak = ((size as i32 * 90) as f64).sqrt() as f32 as i32 as i16;
        w.turn_end = w.turn_peak * 2;
    }
    pw(cx, a).flags |= TURNING;
    p.time = 0;
    true
}

/// Control32::RunProgramB0 (0x412530): the turning program.
fn run_program_b0(cx: &mut Cx, a: Aid, p: &mut EnmList) {
    if pw(cx, a).flags & TURNING != 0 {
        let w = *bw(cx, a);
        if p.time >= w.turn_end as i32 {
            cx.actors.get_mut(a).angle = (w.turn as u16).wrapping_add(w.turn_from) as i16;
            pw(cx, a).flags &= !TURNING;
            cx.enm.state.flags |= 2;
            p.op = 0;
            return;
        }
        let mut turned;
        if p.time >= w.turn_peak as i32 {
            let left = w.turn_end as i32 - p.time;
            turned = (w.turn as i32).abs() - left * left / 180;
            if p.op == 0x20004 {
                cx.enm.state.flags |= 2;
            }
        } else if p.time >= 90 {
            turned = p.time - 45;
            if p.op == 0x20004 {
                let to = angle_to_player(cx, a) as i32;
                let angle = cx.actors.get(a).angle as i32;
                let mut off = ((to - 0x200 - p.arg - angle) & 0x3ff) as i16;
                if off >= 0x200 {
                    off -= 0x400;
                }
                if off > -32 && off < 32 {
                    let done = (angle as u16).wrapping_sub(w.turn_from) as i16;
                    let bw = bw(cx, a);
                    bw.turn_peak = p.time as i16;
                    bw.turn_end = (p.time + 90) as i16;
                    bw.turn = done.wrapping_add(if done < 0 { -45 } else { 45 });
                }
            }
        } else {
            turned = p.time * p.time / 180;
        }
        if bw(cx, a).turn <= 0 {
            turned = -turned;
        }
        let from = bw(cx, a).turn_from;
        cx.actors.get_mut(a).angle = from.wrapping_add(turned as u16) as i16;
        p.time += 1;
        return;
    }
    let angle = cx.actors.get(a).angle;
    let target = match p.op {
        0x20001 => angle as i32 + if cx.rng.rand() & 1 != 0 { 0x200 } else { -0x200 },
        0x20002 => {
            let mut v = angle.wrapping_sub(p.arg as i16) & 0x3ff;
            if v >= 0x200 {
                v -= 0x400;
            }
            v = v.wrapping_add(p.arg as i16);
            cx.actors.get_mut(a).angle = v;
            p.arg
        }
        0x20003 => angle as i32 + ((p.arg - 90) & !0x3ff) + 0x400,
        0x20004 => {
            // `from` is the signed angle (decomp 4d32126); the turn target
            // is truncated to i16, so the extension doesn't change it.
            let from = angle as i32;
            let to = angle_to_player(cx, a) as i32;
            let mut off = ((to - 0x200 - p.arg - from) & 0x3ff) as i16;
            if off >= 0x200 {
                off -= 0x400;
            }
            from + off as i32
        }
        _ => {
            p.time += 1;
            return;
        }
    };
    if !start_turn(cx, a, p, target as i16) {
        p.op = 0;
    }
    p.time += 1;
}

/// Control32::OnChildDestroyed (0x412400).
pub fn body_child_destroyed(cx: &mut Cx, parent: Aid, child: Aid) {
    if cx.actors.get(child).team != 0x61 {
        return;
    }
    let w = bw(cx, parent);
    for arm in &mut w.arms {
        if *arm == child as u32 {
            *arm = 0;
            break;
        }
    }
    let level = pw(cx, child).level as i32;
    let body_level = pw(cx, parent).level as i32;
    let big = level >= 6 || (body_level != 0 && level * 100 / body_level >= 40);
    if big && cx.enm.state.flags & 0x100 != 0 {
        pw(cx, parent).flags &= !DASHING;
        let (x, y) = (cx.actors.get(child).sx(), cx.actors.get(child).sy());
        let angle = cx.actors.angle_to(parent, x, y) as i32 + 0x200;
        cx.enm.list_9c.start(0x10006, angle);
    }
    parts::linked_child_destroyed(cx, parent, child);
    if pw(cx, parent).level <= 1 && cx.enm.state.step_mask & 0x40000 != 0 {
        cx.enm.list_218.start(0x12, 0);
    }
}

/// Control32::OnMove (0x412750): runs the programs of the battle.
pub fn body_move(cx: &mut Cx, a: Aid) -> bool {
    if cx.enm.list_9c.op != 0 {
        let mut p = cx.enm.list_9c;
        run_program_9c(cx, a, &mut p);
        cx.enm.list_9c = p;
    }
    if cx.enm.list_b0.op != 0 {
        let mut p = cx.enm.list_b0;
        run_program_b0(cx, a, &mut p);
        cx.enm.list_b0 = p;
    }
    for side in 0..2 {
        if cx.enm.lists_c4[side].op != 0 {
            let mut p = cx.enm.lists_c4[side];
            run_side_program(cx, a, &mut p, side);
            cx.enm.lists_c4[side] = p;
        }
    }
    true
}

/// Actor::work of control 35.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TurretWork {
    slot: u32,
    burst: i16,
    weight: i16,
    angle: i16,
    ring: i16,
}

/// Control35::OnCreate (0x412950).
pub fn turret_create(cx: &mut Cx, a: Aid, param: Param) -> bool {
    let slot = if let Param::Slot(s) = param { s } else { 0 };
    let act = cx.actors.get_mut(a);
    act.hit_points = 1;
    act.team = 0;
    cx.actors.set_layer(a, 4);
    let depth = cx.enm.slots[slot as usize].depth as i32;
    let weight = ((cx.enm.unknown_98 - depth) as i16) << 2;
    let ring = (depth < (cx.enm.state.max_depth >> 1) as i32) as i16;
    let w = cx.actors.w::<TurretWork>(a);
    w.slot = slot as u32;
    w.weight = weight;
    w.ring = ring;
    w.burst = 0;
    cx.enm.turret_count += 1;
    true
}

/// Control35::StartBurst (0x4127d0).
fn start_burst(cx: &mut Cx, a: Aid) {
    let angle = (cx.rng.rand() & 0x3ff) as i16;
    let (total, count, turrets) = (cx.enm.core_total, cx.enm.core_count, cx.enm.turret_count);
    let w = cx.actors.w::<TurretWork>(a);
    w.angle = angle;
    w.burst = if total != 0 { ((total - count) * w.weight as i32 / total) as i16 } else { 0 };
    w.burst = if turrets != 0 { (w.burst as i32 / turrets) as i16 } else { w.burst };
    if w.burst < 12 {
        w.burst = 12;
    }
}

/// Control35::FireRing (0x412880).
fn fire_ring(cx: &mut Cx, a: Aid) {
    let w = *cx.actors.w::<TurretWork>(a);
    let e = &cx.enm;
    if w.ring != 0 && e.turret_count < 3 && e.core_total != 0 && e.core_count * 100 / e.core_total < 50 {
        let (x, y) = (cx.actors.get(a).sx(), cx.actors.get(a).sy());
        let d = &mut cx.ctl.desc35;
        d.x = x;
        d.y = y;
        d.speed = 4.0;
        d.work = w.slot;
        let mut angle = cx.rng.rand() % 64;
        for _ in 0..16 {
            cx.ctl.desc35.start_angle = angle as u16;
            let d = cx.ctl.desc35;
            spawn(cx, &d);
            cx.play_sound(sounds::FIRE_MS);
            angle += 0x40;
        }
    }
}

/// Control35::OnMove (0x4129c0).
pub fn turret_move(cx: &mut Cx, a: Aid) -> bool {
    let w = *cx.actors.w::<TurretWork>(a);
    if w.burst != 0 {
        if w.burst & 3 == 0 {
            let (x, y) = (cx.actors.get(a).sx(), cx.actors.get(a).sy());
            let d = &mut cx.ctl.desc35;
            d.x = x;
            d.y = y;
            d.start_angle = w.angle as u16;
            d.speed = 6.0;
            d.work = w.slot;
            let d = *d;
            spawn(cx, &d);
            cx.play_sound(sounds::FIRE_BL);
            cx.actors.w::<TurretWork>(a).angle += 20;
        }
        cx.actors.w::<TurretWork>(a).burst -= 1;
        return true;
    }
    match cx.actors.get(a).age % 360 {
        60 | 120 | 180 => start_burst(cx, a),
        240 | 270 | 300 => fire_ring(cx, a),
        _ => {}
    }
    true
}

#[allow(unused)]
fn unused() -> (Aid, f32) {
    (NIL, cos(0))
}
