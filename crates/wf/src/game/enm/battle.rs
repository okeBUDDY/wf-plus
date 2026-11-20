//! The battle (enm_battle.cpp, enm_tactic_helpers.cpp): starting it and
//! running the tactics every frame.

use super::{state_flags, tactics};
use crate::game::Cx;
use crate::game::actor::{self, Aid, Param, controls, parts};

/// CEnmParam::StartBattle (0x417170): creates the planned boss.
pub fn start_battle(cx: &mut Cx, player: Option<Aid>) -> Option<Aid> {
    let e = &mut cx.enm;
    e.record_count = 0;
    for i in 0..e.slot_count.max(0) as usize {
        e.slots[i].flags &= 0xffff;
    }
    e.turn_speed = e.state.aim_speed as i32 * 2 / 1024 + 1;
    e.fire_rate = e.state.fire_speed as i32 * 50 / 1024 + 10;
    e.state.flags &= 0xffff_0000;
    e.fire_wait = [0, 0];
    e.turret_count = 0;
    e.turret_weight = (e.state.max_depth as i32 * 2).max(5);
    e.battle_time = 0;
    e.aimed_frames = 0;
    e.deaths = 0;
    e.dir_frames = [0; 3];
    e.score_chain = 0;
    e.score_rapid = 0;
    e.score_base = 0;
    e.rapid_count = 0;
    e.chain_count = 0;
    e.unused_17c = 0;
    e.player_samples = 0;
    e.player_travel = 0.0;
    if let Some(p) = player {
        let p = cx.actors.get(p);
        cx.enm.player_pos = [p.x(), p.y()];
    }
    let e = &mut cx.enm;
    for i in 0..14 {
        e.tally[i] = 0;
    }
    e.move_program.op = 0;
    e.turn_program.op = 0;
    e.fire_programs[0].op = 0;
    e.fire_programs[1].op = 0;
    e.tactic_program.start(0, 0);
    let core = e.find_slot(super::SIDE_MASK);
    e.core_count = 0;
    e.parts_alive = [0, 0];
    e.armored_parts = [0, 0];
    e.transform_parts = [0, 0];
    let boss = actor::create(cx, -i32::from(controls::ids::BOSS_BODY), None, cx.arena.center_x(), -480.0, 4, 0, core.map_or(Param::None, Param::Slot));
    let e = &mut cx.enm;
    e.parts_alive[0] -= 1;
    e.core_total = e.core_count;
    e.start_parts = e.parts_alive;
    e.trans_count = 0;
    boss
}

/// CEnmParam::IsSideBeaten (0x416170).
pub fn is_side_beaten(cx: &Cx, percent: i32) -> bool {
    let e = &cx.enm;
    if e.state.tactic_mask & 2 != 0 {
        let left = if e.start_parts[0] != 0 { e.parts_alive[0] * 100 / e.start_parts[0] } else { 0 };
        let right = if e.start_parts[1] != 0 { e.parts_alive[1] * 100 / e.start_parts[1] } else { 0 };
        if e.player_dir == 2 {
            if left > right && right <= percent {
                return true;
            }
        } else if right > left && left <= percent {
            return true;
        }
    }
    false
}

/// CEnmParam::GlowArmor (0x4173a0).
pub fn glow_armor(cx: &mut Cx, boss: Aid, time: i32) -> bool {
    if cx.enm.armored_parts[0] <= 0 && cx.enm.armored_parts[1] <= 0 {
        return false;
    }
    cx.enm.glow_time = time;
    controls::send_event(cx, boss, parts::events::GLOW, 0x110a);
    true
}

/// CEnmParam::StartTransform (0x4173f0).
pub fn start_transform(cx: &mut Cx, boss: Aid) -> bool {
    if cx.enm.transform_parts[0] <= 0 && cx.enm.transform_parts[1] <= 0 {
        return false;
    }
    controls::send_event(cx, boss, parts::events::TRANSFORM, 0xffff_310au32 as i32);
    true
}

/// CEnmParam::RunBattle (0x4178f0).
pub fn run_battle(cx: &mut Cx, player: Option<Aid>, boss: Option<Aid>) {
    let Some(boss) = boss else { return };
    let (x, y);
    if let Some(p) = player {
        let p = cx.actors.get(p);
        x = p.x();
        y = p.y();
        let e = &mut cx.enm;
        if e.battle_time & 0x1f == 0 {
            e.player_samples += 1;
            let dx = x - e.player_pos[0];
            let dy = y - e.player_pos[1];
            e.player_pos = [x, y];
            e.player_travel += (dx * dx + dy * dy).sqrt();
        }
    } else {
        x = cx.enm.player_pos[0];
        y = cx.enm.player_pos[1];
    }
    let angle = (cx.actors.angle_to(boss, x, y) as i32 - cx.actors.get(boss).angle as i32) & 0x3ff;
    let e = &mut cx.enm;
    e.player_dir = if (0x1c0..=0x240).contains(&angle) {
        0
    } else if (0x100..=0x300).contains(&angle) {
        1
    } else {
        2
    };
    e.battle_time += 1;
    e.dir_frames[e.player_dir as usize] += 1;
    if e.state.flags & state_flags::AIMED != 0 {
        e.aimed_frames += 1;
    }
    if e.state.flags & state_flags::BATTLE != 0 {
        let over = tactics::run(cx, boss);
        if over {
            let next;
            if cx.enm.state.tactic_count <= 1 {
                next = 0;
            } else if is_side_beaten(cx, 0) {
                next = 1;
            } else {
                let mut n;
                loop {
                    let e = &cx.enm;
                    let pick = cx.rng.rand() % e.state.tactic_weight.max(1) as i32;
                    n = 0;
                    for i in 0..e.state.tactic_count as usize {
                        n = e.state.tactics[i][0] as i32;
                        if pick < e.state.tactics[i][1] as i32 {
                            break;
                        }
                    }
                    if n != cx.enm.tactic_program.op {
                        break;
                    }
                }
                next = n;
            }
            cx.enm.tactic_program.start(next, 0);
        } else {
            cx.enm.tactic_program.time -= 1;
        }
    }
    let e = &mut cx.enm;
    for side in 0..2 {
        if e.fire_wait[side] > 0 {
            e.fire_wait[side] -= e.fire_rate;
            if e.fire_wait[side] < 0 {
                e.fire_wait[side] = 0;
            }
        }
    }
    e.state.flags &= !0xff;
}
