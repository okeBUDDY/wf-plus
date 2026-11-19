//! The parts of the boss (actor_control.cpp): ScoredControl, PartControl,
//! LinkedPartControl (controls 32..34) and CorePartControl (40..49).

use super::controls::{self, CORE_CONTROLS, DEBRIS_CONTROLS};
use super::{ActorDesc, Aid, Param, create, flags, sheet, spawn};
use crate::game::Cx;
use crate::game::enm::{self, add_clamped, part_flags, side_of};
use crate::game::sounds;
use bytemuck::{Pod, Zeroable};

/// Actor::work of the parts of the boss (PartWork, 0x30..0x54).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct PartWork {
    /// The plan slot (index, 0: none).
    pub slot: u32,
    pub flags: u32,
    /// What turns: the part itself or its parent.
    pub turret: u32,
    pub max_hit_points: i32,
    pub level: i16,
    pub low: i16,
    pub high: i16,
    pub angle: i16,
    pub state: i32,
    pub state_time: i16,
    pub unknown_1e: i16,
    pub rank: i16,
    pub unknown_22: i16,
}

/// PartWork::Flags.
pub const ALIVE: u32 = 0x100_0000;
pub const TRANS: u32 = 0x400_0000;

impl PartWork {
    pub fn enter(&mut self, state: i32) {
        self.state = state;
        self.unknown_1e = 0;
        self.state_time = 0;
    }
}

pub fn pw(cx: &mut Cx, a: Aid) -> &mut PartWork {
    cx.actors.w::<PartWork>(a)
}

fn slot_of(cx: &mut Cx, a: Aid) -> u16 {
    pw(cx, a).slot as u16
}

/// The (constant) level of a core control (GetLevel).
pub fn core_level(id: i16) -> i32 {
    match id {
        40 => 0,
        41 => 1,
        42 => 2,
        44 => 3,
        46 => 4,
        47 => 5,
        _ => 6,
    }
}

/// ScoredControl::GetScore.
pub fn get_score(cx: &mut Cx, id: i16, a: Aid) -> i32 {
    match id {
        32 => (cx.global.stage * 10 + cx.enm.state.unknown_24 as i32 - 10) * 1000,
        33 | 34 => {
            let s = slot_of(cx, a);
            cx.enm.part(s).score
        }
        40 => 1000,
        41 | 42 => 1200,
        44 | 47 => 2000,
        _ => 2500,
    }
}

/// ScoredControl::GetHitPoints.
pub fn get_hit_points(cx: &mut Cx, id: i16, a: Aid) -> i32 {
    match id {
        32 => 1,
        33 | 34 => {
            let s = slot_of(cx, a);
            cx.enm.part(s).hit_points as i32
        }
        40 => 20,
        41 | 42 => 30,
        44..=47 if id != 45 => 40,
        45 => 2,
        48 => 5,
        _ => 40,
    }
}

/// ScoredControl::Prepare (0x418c40).
pub fn prepare(cx: &mut Cx, id: i16, a: Aid, slot: u16) {
    let w = pw(cx, a);
    w.slot = slot as u32;
    w.flags = 0;
    let hp = get_hit_points(cx, id, a);
    let hp = hp + hp * cx.enm.state.unknown_26 as i32 / 1024;
    cx.actors.get_mut(a).hit_points = hp;
}

/// ScoredControl::IsOffScreen (0x4146a0).
pub fn is_off_screen(cx: &Cx, a: Aid) -> bool {
    let x = cx.actors.get(a);
    x.sx() < 0.0 || x.sx() > 640.0 || x.sy() < 0.0 || x.sy() > 480.0
}

/// PartControl::Schedule (0x418c90): waits `delay`, then OnDamage(`next`).
pub fn schedule(cx: &mut Cx, a: Aid, delay: i32, next: i32) {
    on_damage(cx, a, 1);
    let w = pw(cx, a);
    w.unknown_1e = delay as i16;
    w.state_time = next as i16;
}

/// PartControl::ScaleHitPoints (0x418d70).
pub fn scale_hit_points(cx: &mut Cx, a: Aid) {
    let level = pw(cx, a).level as i32;
    let x = cx.actors.get_mut(a);
    if x.team != 0xa1 {
        x.hit_points = (level + 3) * x.hit_points / 4;
    }
}

/// PartControl::InitPart (0x419070).
pub fn init_part(cx: &mut Cx, id: i16, a: Aid, slot: u16) {
    prepare(cx, id, a, slot);
    let part = *cx.enm.part(slot);
    let s = cx.enm.slots[slot as usize];
    let parent = cx.enm.find_slot(enm::parent_code(s.code));
    let (team, level) = if part.item.flags & part_flags::GUARD != 0 { (0xa1, 0) } else { (0x61, 1) };
    cx.actors.get_mut(a).team = team;
    let actor_parent = cx.actors.get(a).parent;
    let turret_parent = parent.filter(|&p| cx.enm.part(p).item.flags & part_flags::TURRET != 0 && s.code & 7 == 0);
    let w = pw(cx, a);
    w.level = level;
    if let Some(p) = turret_parent {
        let ps = cx.enm.slots[p as usize];
        let w = pw(cx, a);
        w.turret = actor_parent as u32;
        w.low = ps.low;
        w.high = ps.high;
    } else {
        w.turret = a as u32;
        w.low = s.low;
        w.high = s.high;
    }
    let w = pw(cx, a);
    w.angle = if part.item.flags & part_flags::AIM_HIGH != 0 { w.high } else { w.low };
    if s.flags & 3 == 1 {
        w.flags |= 0x10000;
    } else if part.item.flags & 0x20 != 0 {
        w.flags |= 0x20000;
    } else if part.item.flags & 0x10 != 0 {
        w.flags |= 0x10000;
    }
    let (turret, angle) = (w.turret as Aid, w.angle as u16);
    let t = cx.actors.get_mut(turret);
    t.angle = angle as i16;
    t.start_angle = angle;
    let hp = cx.actors.get(a).hit_points;
    let w = pw(cx, a);
    w.max_hit_points = hp;
    w.flags |= ALIVE;
    w.state = 0;
    let wflags = w.flags;
    let side = side_of(s.code);
    cx.enm.parts_alive[side] += 1;
    if wflags & 0x20000 != 0 {
        cx.enm.unknown_104[side] += 1;
    }
    if s.flags & 8 != 0 {
        cx.enm.armored_parts[side] += 1;
        pw(cx, a).flags |= 0x40000;
    }
}

/// CorePartControl::InitCore (0x419220).
pub fn init_core(cx: &mut Cx, id: i16, a: Aid, slot: u16) {
    init_part(cx, id, a, slot);
    let power = cx.enm.slots[slot as usize].power as i32;
    let w = pw(cx, a);
    w.rank = (power * 100 / 1024) as i16;
    w.unknown_22 = 0;
    let rank = w.rank as i32;
    super::cores::setup(cx, id, a, rank);
    let side = side_of(cx.enm.slots[slot as usize].code);
    *cx.enm.core_power_mut(side, core_level(id) as usize) += rank + 50;
    cx.enm.core_count += 1;
}

/// LinkedPartControl::InitLinked (0x4192b0).
pub fn init_linked(cx: &mut Cx, id: i16, a: Aid, slot: u16) {
    init_part(cx, id, a, slot);
    let part = *cx.enm.part(slot);
    let color = cx.enm.state.color;
    {
        let x = cx.actors.get_mut(a);
        x.debris = DEBRIS_CONTROLS[(part.item.debris as usize) & 3];
        x.color = color;
        if part.item.flags & part_flags::MIRROR_X != 0 {
            x.flags |= flags::MIRROR_X;
            x.matrix_angle = x.matrix_angle.wrapping_add(0x1000);
        }
        if part.item.flags & part_flags::MIRROR_Y != 0 {
            x.flags |= flags::MIRROR_Y;
            x.matrix_angle = x.matrix_angle.wrapping_add(0x1000);
        }
    }
    let sprite = cx.pats.get(sheet::ENM + part.item.pat_file as u8, part.item.pat as i32);
    cx.actors.set_sprite(a, sprite);
    on_damage(cx, a, if part.item.flags & 0x10 != 0 { 6 } else { 0 });
}

/// The part's events from the body and the tactics
/// (LinkedPartControl::OnEvent 0x419370).
pub fn linked_event(cx: &mut Cx, a: Aid, x: i32, y: i32) {
    let mut count = y & 0xff;
    let alive = pw(cx, a).flags & ALIVE != 0;
    if y & 0x1000 != 0 && (y & 0x2000 != 0 || alive) && (y & 0x100 != 0 || count == 0) {
        on_damage(cx, a, x);
    }
    count -= 1;
    if count >= 0 {
        let y = (y & !0xff) | count;
        let mut child = cx.actors.get(a).first_child;
        while child != super::NIL {
            controls::send_event(cx, child, x, y);
            child = cx.actors.get(child).prev_sibling;
        }
    }
}

/// CorePartControl::OnEvent (0x418cc0).
pub fn core_event(cx: &mut Cx, id: i16, a: Aid, x: i32, y: i32) {
    let w = *pw(cx, a);
    if y & 0x2000 == 0 && w.flags & ALIVE == 0 {
        return;
    }
    if y & (0x10000 << core_level(id)) == 0 {
        return;
    }
    if y & 0x100 == 0 && y & 0xff != 0 {
        return;
    }
    if x == 5 {
        if y & 0x800 != 0 {
            if w.flags & 0x200_0000 == 0 {
                super::cores::setup(cx, id, a, 100);
                pw(cx, a).flags |= 0x200_0000;
            }
        } else if w.flags & 0x200_0000 != 0 {
            super::cores::setup(cx, id, a, w.rank as i32);
            pw(cx, a).flags &= !0x200_0000;
        }
        let u22 = pw(cx, a).unknown_22 as i32;
        if u22 > cx.ctl.max_unknown_22 {
            cx.ctl.max_unknown_22 = u22;
        }
    }
    on_damage(cx, a, x);
}

/// LinkedPartControl::OnChildDestroyed (0x419400).
pub fn linked_child_destroyed(cx: &mut Cx, parent: Aid, child: Aid) {
    if cx.actors.get(child).team != 0x61 {
        return;
    }
    let level = pw(cx, parent).level;
    if cx.actors.get(parent).team != 0xa1 && level > 1 {
        let old = level as i32;
        let child_level = pw(cx, child).level;
        let w = pw(cx, parent);
        w.level -= child_level;
        let new = w.level as i32;
        let (slot, new_level) = (w.slot, w.level);
        let p = cx.actors.get_mut(parent);
        p.hit_points = p.hit_points.abs() * (new + 3) / (old + 3);
        if p.hit_points < 1 {
            p.hit_points = 1;
        }
        if new_level == 1 {
            create(cx, 35, Some(parent), 0.0, 0.0, 4, 0, Param::Slot(slot as u16));
        }
    }
    if cx.actors.get(parent).id != 32 {
        let pp = cx.actors.get(parent).parent;
        let pid = cx.actors.get(pp).id;
        controls::on_child_destroyed(cx, pid, pp, child);
    }
}

/// LinkedPartControl::SpawnChildren (0x418da0).
pub fn spawn_children(cx: &mut Cx, a: Aid, base: u32, mut index: u32, mut count: i32) -> bool {
    while count > 0 {
        let Some(s) = cx.enm.find_slot(base | index) else { return false };
        let slot = cx.enm.slots[s as usize];
        let part = *cx.enm.part(s);
        let child = match slot.flags & 3 {
            0 => create(cx, CORE_CONTROLS[(slot.weapon as usize) % 7], Some(a), part.item.x as f32, part.item.y as f32, 6, 0, Param::Slot(s)),
            1 => create(cx, CORE_CONTROLS[(slot.weapon as usize) % 7], Some(a), part.x1 as f32, part.y1 as f32, 6, 0, Param::Slot(s)),
            _ => create(cx, 34, Some(a), part.item.x as f32, part.item.y as f32, 4, 0, Param::Slot(s)),
        };
        if let Some(c) = child {
            let l = pw(cx, c).level;
            pw(cx, a).level += l;
        }
        count -= 1;
        index += 1;
    }
    true
}

/// A sprite spawned on a part (PartPiece).
pub struct PartPiece {
    pub x: f32,
    pub y: f32,
    pub layer: i8,
    pub debris: i8,
    pub pat: i16,
    pub sheet: u8,
}

/// PartControl::SpawnPieces (0x418f50).
pub fn spawn_pieces(cx: &mut Cx, a: Aid, pieces: &[PartPiece]) {
    let mut desc = ActorDesc { work: cx.actors.get(a).work[0], color: 0xffff_ffff, ..Default::default() };
    for p in pieces {
        desc.debris = DEBRIS_CONTROLS[p.debris as usize];
        desc.flags = 0;
        desc.team = 0xa1;
        desc.hit_points = 1;
        desc.x = p.x;
        desc.sprite = cx.pats.get(p.sheet, p.pat as i32);
        desc.parent = Some(a);
        desc.y = p.y;
        desc.layer = (cx.actors.get(a).layer as i32 + p.layer as i32) as u16;
        spawn(cx, &desc);
    }
}

/// PartControl::SpawnItems (0x4194a0).
pub fn spawn_items(cx: &mut Cx, a: Aid) {
    let s = slot_of(cx, a);
    let part = *cx.enm.part(s);
    let mut desc = ActorDesc { color: 0xffff_ffff, work: s as u32, ..Default::default() };
    for i in 0..part.item_count.max(0) as usize {
        let Some(item) = cx.enm.plan.items.get(part.items as usize + i).copied() else { break };
        desc.debris = DEBRIS_CONTROLS[(item.debris as usize) & 3];
        desc.flags = 0;
        desc.team = 0xa1;
        desc.hit_points = 1;
        desc.sprite = cx.pats.get(sheet::ENM + item.pat_file as u8, item.pat as i32);
        desc.parent = Some(a);
        desc.x = item.x as f32;
        desc.y = item.y as f32;
        desc.layer = 5;
        spawn(cx, &desc);
    }
}

/// PartControl::PlayTrans (0x419020).
fn play_trans(cx: &mut Cx, a: Aid) {
    if pw(cx, a).flags & TRANS == 0 {
        pw(cx, a).flags |= TRANS;
        cx.enm.trans_count += 1;
        cx.play_sound_on(27, sounds::TRANS1);
    }
}

/// PartControl::StopTrans (0x419580).
fn stop_trans(cx: &mut Cx, a: Aid, next: bool) {
    if pw(cx, a).flags & TRANS != 0 {
        pw(cx, a).flags &= !TRANS;
        cx.enm.trans_count -= 1;
        if cx.enm.trans_count <= 0 {
            cx.sound.stop_channel(27);
        }
        if next {
            cx.play_sound_on(28, sounds::TRANS2);
        }
    }
}

/// PartControl::OnDestroy (0x4195f0): scores the part.
pub fn part_destroy(cx: &mut Cx, a: Aid) {
    let id = cx.actors.get(a).id;
    let s = slot_of(cx, a);
    let angle = cx.actors.get(a).angle;
    cx.enm.slots[s as usize].lerp_at = angle;
    let mut chain = 1i32;
    let mut rapid = 1i32;
    let mut anc = a;
    loop {
        anc = cx.actors.get(anc).parent;
        if anc == super::NIL {
            chain >>= 1;
            break;
        }
        if cx.actors.get(anc).flags != flags::DEAD {
            break;
        }
        chain <<= 1;
    }
    let w = *pw(cx, a);
    if chain > 32 {
        chain = 32;
    } else if chain == 1 {
        let x = cx.actors.get(a);
        let speed = w.max_hit_points * 100 / (x.age as i32 - x.hit_age as i32 + 1);
        rapid = chain;
        if speed > 110 {
            rapid <<= speed / 55 - 1;
            cx.enm.slots[s as usize].hits += 1;
            if rapid > 32 {
                rapid = 32;
            }
        }
    }
    if w.flags & TRANS != 0 {
        pw(cx, a).flags &= !TRANS;
        cx.enm.trans_count -= 1;
        if cx.enm.trans_count <= 0 {
            cx.sound.stop_channel(27);
        }
    }
    cx.enm.log_destroy(s, chain, rapid);
    if chain == 1 {
        add_clamped(&mut cx.enm.slots[s as usize].power, 20);
        cx.game.on_part_destroyed();
    }
    let side = side_of(cx.enm.slots[s as usize].code);
    cx.enm.parts_alive[side] -= 1;
    if w.flags & 0x20000 != 0 {
        cx.enm.unknown_104[side] -= 1;
    }
    if w.flags & 0x40000 != 0 {
        cx.enm.armored_parts[side] -= 1;
    }
    let score = get_score(cx, id, a);
    cx.enm.score_base += score;
    if rapid > 1 {
        cx.enm.rapid_count += 1;
        cx.enm.score_rapid += (rapid - 1) * score;
    }
    if chain > 1 {
        cx.enm.chain_count += 1;
        cx.enm.score_chain += (chain - 1) * score;
    }
    cx.global.entry.score = cx.global.entry.score.wrapping_add((score * rapid * chain) as u32);
}

/// CorePartControl::OnDestroy (0x4197d0).
pub fn core_destroy(cx: &mut Cx, id: i16, a: Aid) {
    part_destroy(cx, a);
    let w = *pw(cx, a);
    let s = w.slot as usize;
    let side = side_of(cx.enm.slots[s].code);
    *cx.enm.core_power_mut(side, core_level(id) as usize) -= w.rank as i32 + 50;
    cx.enm.core_count -= 1;
    let slot = cx.enm.slots[s];
    let level = &mut cx.enm.state.weapon_levels[(slot.weapon as u8 as usize).min(6)];
    if slot.power > *level {
        *level = slot.power;
    }
}

/// PartControl::SpawnFlare (0x419850).
fn spawn_flare(cx: &mut Cx, a: Aid) {
    let Some(boss) = cx.ctl.boss else { return };
    let (sx, sy) = (cx.actors.get(a).sx(), cx.actors.get(a).sy());
    let mut angle = cx.actors.angle_to(boss, sx, sy) as u16;
    let offset = ((cx.enm.unknown_158 as u16).wrapping_sub(angle) & 0x3ff) as i16;
    if offset > 0x100 && offset < 0x300 {
        let x = cx.actors.get(a);
        if x.relative_tf {
            angle = cx.actors.to_local_angle(x.parent, angle);
            let f = cx.actors.get(a).flags;
            if f & flags::MIRROR_X != 0 {
                angle = angle.wrapping_neg();
            }
            if f & flags::MIRROR_Y != 0 {
                angle = 0x200u16.wrapping_sub(angle);
            }
        }
        create(cx, 23, Some(a), 0.0, 0.0, 1, angle, Param::None);
        cx.enm.unknown_164 += 1;
    }
}

/// PartControl::OnDamage (0x4198e0): the part's state machine.
pub fn on_damage(cx: &mut Cx, a: Aid, mut how: i32) {
    stop_trans(cx, a, true);
    let w = pw(cx, a);
    if w.state == 1 {
        w.state = 0;
        w.flags |= ALIVE;
    }
    let wf = w.flags;
    match how {
        6 | 0 => {
            if how == 6 && wf & 0x10000 == 0 {
                return;
            }
            let w = pw(cx, a);
            w.flags |= ALIVE;
            w.enter(how);
            return;
        }
        5 => {
            if is_off_screen(cx, a) {
                return;
            }
            let s = slot_of(cx, a);
            add_clamped(&mut cx.enm.slots[s as usize].power, 3);
        }
        7 => {
            if wf & 0x10000 == 0 {
                return;
            }
        }
        4 => {
            if wf & 0x20000 == 0 {
                return;
            }
            let w = pw(cx, a);
            if w.flags & 0x800_0000 != 0 {
                w.flags &= !0x800_0000;
                how = 3;
            } else {
                w.flags |= 0x800_0000;
            }
            play_trans(cx, a);
        }
        8 => {
            if wf & 0x40000 == 0 {
                return;
            }
        }
        9 => {
            let s = slot_of(cx, a);
            if cx.enm.part(s).item.flags & 0x200 != 0 {
                spawn_flare(cx, a);
            }
            return;
        }
        _ => {}
    }
    let w = pw(cx, a);
    w.flags &= !ALIVE;
    w.enter(how);
}

/// Turns the turret toward `goal` by the turn speed; true when it is there.
fn turn_toward(cx: &mut Cx, turret: Aid, goal: i16) -> bool {
    let speed = cx.enm.turn_speed;
    let now = cx.actors.get(turret).angle;
    let mut offset = goal.wrapping_sub(now) & 0x3ff;
    if offset >= 0x200 {
        offset -= 0x400;
    }
    let t = cx.actors.get_mut(turret);
    if (offset.abs() as i32) < speed {
        t.angle = now.wrapping_add(offset);
        true
    } else {
        t.angle = if offset < 0 { now.wrapping_sub(speed as i16) } else { now.wrapping_add(speed as i16) };
        false
    }
}

/// PartControl::MovePart (0x419a70): runs the states OnDamage sets.
pub fn move_part(cx: &mut Cx, a: Aid) -> bool {
    let w = *pw(cx, a);
    let turret = w.turret as Aid;
    match w.state {
        1 => {
            let w = pw(cx, a);
            if w.unknown_1e > 0 {
                w.unknown_1e -= 1;
                if w.unknown_1e == 0 {
                    let next = w.state_time as i32;
                    on_damage(cx, a, next);
                }
            }
        }
        6 | 7 => {
            let old = cx.actors.get(turret).angle;
            let player = cx.game.player;
            let speed = cx.enm.turn_speed as i16;
            cx.actors.turn_to(turret, player, speed);
            let t = cx.actors.get_mut(turret);
            t.angle = super::clamp_angle(t.angle as u16, w.low, w.high) as i16;
            if old == t.angle {
                cx.enm.state.flags |= 1;
                if w.state == 7 {
                    on_damage(cx, a, 5);
                }
            }
        }
        3 => match w.state_time {
            0 => {
                if turn_toward(cx, turret, w.angle) {
                    stop_trans(cx, a, true);
                    pw(cx, a).state_time += 1;
                }
                let w = pw(cx, a);
                if w.unknown_1e > 0 {
                    w.unknown_1e -= 1;
                }
            }
            1 => {
                let w = pw(cx, a);
                w.unknown_1e -= 1;
                if w.unknown_1e <= 0 {
                    on_damage(cx, a, 0);
                }
            }
            _ => {}
        },
        4 => {
            if w.state_time == 0 {
                pw(cx, a).state_time += 1;
            }
            if pw(cx, a).state_time == 1 {
                let s = w.slot as u16;
                let goal = if cx.enm.part(s).item.flags & part_flags::AIM_HIGH != 0 { w.low } else { w.high };
                if turn_toward(cx, turret, goal) {
                    stop_trans(cx, a, true);
                    on_damage(cx, a, 0);
                }
            }
        }
        _ => {}
    }
    true
}

/// LinkedPartControl::OnMove (0x419cc0): state 8 makes the part glow.
pub fn linked_move(cx: &mut Cx, a: Aid) -> bool {
    if pw(cx, a).state != 8 {
        return move_part(cx, a);
    }
    let color = cx.enm.state.color;
    if pw(cx, a).state_time == 0 {
        if cx.enm.glow_time < 120 {
            cx.enm.glow_time = 120;
        }
        let gt = cx.enm.glow_time;
        create(cx, 22, Some(a), 0.0, 0.0, 5, 0, Param::Int(gt));
        let mut child = cx.actors.get(a).first_child;
        while child != super::NIL {
            let empty = match cx.actors.get(child).sprite {
                super::Obj::Pat(p) => cx.pats.info(p).is_some_and(|i| i.empty),
                _ => false,
            };
            if empty {
                create(cx, 22, Some(child), 0.0, 0.0, 5, 0, Param::Int(gt));
            }
            child = cx.actors.get(child).prev_sibling;
        }
        cx.play_sound(sounds::BARRIER);
        let w = pw(cx, a);
        w.state_time += 1;
        w.unknown_1e = 0;
    }
    match pw(cx, a).state_time {
        1 => {
            let w = pw(cx, a);
            w.unknown_1e += 1;
            let t = w.unknown_1e as i32;
            cx.actors.get_mut(a).lerp_color(color, 0xffff_ff00, t, 60);
            if t >= 60 {
                let x = cx.actors.get_mut(a);
                if x.team == 0x61 {
                    x.team = 0xe1;
                }
                let gt = cx.enm.glow_time;
                let w = pw(cx, a);
                w.unknown_1e = (gt - 120) as i16;
                w.state_time += 1;
            }
        }
        2 => {
            let w = pw(cx, a);
            w.unknown_1e -= 1;
            if w.unknown_1e <= 0 {
                w.state_time += 1;
            }
        }
        3 => {
            let w = pw(cx, a);
            w.unknown_1e += 1;
            let t = w.unknown_1e as i32;
            cx.actors.get_mut(a).lerp_color(0xffff_ff00, color, t, 60);
            if t >= 60 {
                let x = cx.actors.get_mut(a);
                if x.team == 0xe1 {
                    x.team = 0x61;
                }
                on_damage(cx, a, 0);
            }
        }
        _ => {}
    }
    true
}
