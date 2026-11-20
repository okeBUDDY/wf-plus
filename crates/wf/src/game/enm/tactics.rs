//! The battle tactics run by tactic_program (enm_tactics.cpp,
//! enm_tactic_helpers.cpp RunOpening). Each returns true when it is over.

use super::battle::{glow_armor, is_side_beaten, start_transform};
use super::{ops, state_flags};
use crate::game::Cx;
use crate::game::actor::Aid;

/// Dispatch of CEnmParam::RunBattle's switch on tactic_program.op.
pub fn run(cx: &mut Cx, boss: Aid) -> bool {
    match cx.enm.tactic_program.op {
        0 => tactic_roam(cx),
        1 => tactic_dash_flip(cx),
        2 => tactic_charge(cx),
        3 => tactic_rear_sparks(cx),
        4 => tactic_stream(cx),
        5 => tactic_rapid_fire(cx),
        6 => tactic_glow_armor(cx, boss),
        7 => tactic_spin_dash(cx),
        8 => tactic_transform(cx, boss),
        18 => tactic_last_stand(cx),
        9 => tactic_charge_rear_sparks(cx),
        10 => tactic_charge_flip(cx),
        11 => tactic_repeat_charge(cx),
        12 | 13 => true,
        14 => tactic_spin(cx),
        15 => tactic_spin_glow_sparks(cx, boss),
        16 => tactic_glow_rapid_fire(cx, boss),
        17 => tactic_spin_ram(cx, boss),
        _ => false,
    }
}

fn both(cx: &mut Cx, op: i32, arg: i32) {
    cx.enm.fire_programs[0].start(op, arg);
    cx.enm.fire_programs[1].start(op, arg);
}

/// CEnmParam::TacticRoam (0x4161d0), tactic 0: dashes about at random, now and
/// then at the player.
fn tactic_roam(cx: &mut Cx) -> bool {
    match cx.enm.tactic_program.step {
        0 => {
            both(cx, ops::FIRE_ANY, 100);
            cx.enm.tactic_program.arg = if cx.enm.state.tactic_count > 5 { 1 } else { cx.rng.rand() % 3 + 1 };
            cx.enm.move_program.start(ops::MOVE_RANDOM, 0);
            cx.enm.tactic_program.step += 1;
        }
        1
            if cx.enm.move_program.op == 0 => {
                if is_side_beaten(cx, 0) {
                    return true;
                }
                cx.enm.tactic_program.arg -= 1;
                if cx.enm.tactic_program.arg <= 0 {
                    return true;
                }
                if cx.rng.rand() & 7 != 0 {
                    cx.enm.move_program.start(ops::MOVE_RANDOM, 0);
                } else {
                    cx.enm.move_program.start(ops::MOVE_RAM, 0);
                }
            }
        _ => {}
    }
    false
}

/// CEnmParam::TacticDashFlip (0x4162f0), tactic 1: dashes about; turns half
/// round when a side is beaten to 50%.
fn tactic_dash_flip(cx: &mut Cx) -> bool {
    match cx.enm.tactic_program.step {
        0 => {
            both(cx, ops::FIRE_ANY, 70);
            cx.enm.move_program.start(ops::MOVE_RANDOM, 0);
            if is_side_beaten(cx, 50) {
                cx.enm.turn_program.start(ops::TURN_FLIP, 0);
            }
            cx.enm.tactic_program.step += 1;
        }
        1 => {
            let e = &mut cx.enm;
            if e.move_program.op == 0 && e.turn_program.op == 0 {
                return true;
            }
            if e.move_program.op == 0 {
                e.move_program.start(ops::MOVE_RANDOM, 0);
            }
        }
        _ => {}
    }
    false
}

/// CEnmParam::RunOpening (0x4163f0): steps 0..3, then `next_step`.
fn run_opening(cx: &mut Cx, next_step: i32) {
    let e = &mut cx.enm;
    if e.tactic_program.step >= next_step {
        return;
    }
    let mut step = e.tactic_program.step;
    if step == 0 {
        e.fire_programs[0].start(ops::FIRE_ANY, 70);
        e.fire_programs[1].start(ops::FIRE_ANY, 70);
        e.tactic_program.arg = 1;
        e.turn_program.start(ops::TURN_FACE, 0);
        e.tactic_program.step += 1;
        step = 1;
    }
    match step {
        1 => {
            if e.turn_program.op != 0 && e.state.flags & state_flags::TURNED == 0 {
                return;
            }
            if e.player_dir == 2 && e.tactic_program.arg > 0 {
                if e.turn_program.op == 0 {
                    e.tactic_program.arg -= 1;
                    e.turn_program.start(ops::TURN_FACE, 0);
                }
                return;
            }
            e.fire_programs[0].op = 0;
            e.fire_programs[1].op = 0;
            e.move_program.start(ops::MOVE_CHARGE, 0);
            e.tactic_program.step += 1;
        }
        2 => {
            if e.move_program.op == 0 && e.turn_program.op == 0 {
                e.tactic_program.wait(60);
            }
        }
        3
            if e.tactic_program.time <= 0 => {
                e.tactic_program.step = next_step;
            }
        _ => {}
    }
}

/// CEnmParam::TacticCharge (0x416560), tactic 2: the opening, then a dash and a
/// turn back.
fn tactic_charge(cx: &mut Cx) -> bool {
    run_opening(cx, 10);
    let e = &mut cx.enm;
    match e.tactic_program.step {
        10 => {
            e.fire_programs[0].start(ops::FIRE_ANY, 100);
            e.fire_programs[1].start(ops::FIRE_ANY, 100);
            e.move_program.start(ops::MOVE_RANDOM, 0);
            e.turn_program.start(ops::TURN_TO, 0);
            e.tactic_program.step += 1;
        }
        11
            if e.move_program.op == 0 && e.turn_program.op == 0 => {
                return true;
            }
        _ => {}
    }
    false
}

/// CEnmParam::TacticRearSparks (0x416630), tactic 3: turns its back on the
/// player and fires the sparks of side 1 boosted, 3 to 5 times.
fn tactic_rear_sparks(cx: &mut Cx) -> bool {
    let mut step = cx.enm.tactic_program.step;
    if step == 0 {
        cx.enm.fire_programs[0].start(ops::FIRE_ANY, 50);
        cx.enm.fire_programs[1].op = 0;
        let arg = cx.rng.rand() % 3 + 3;
        cx.enm.tactic_program.set_arg_and_next(arg);
        step = 1;
    }
    let e = &mut cx.enm;
    match step {
        1 => {
            if e.core_power(1, 4) == 0 {
                return true;
            }
            e.turn_program.start(ops::TURN_FACE, 0x200);
            e.tactic_program.step += 1;
        }
        2 => {
            if e.turn_program.op == 0 || e.state.flags & state_flags::TURNED != 0 {
                e.fire_programs[1].start(ops::FIRE_BOOSTED, 4);
                e.tactic_program.wait(60);
            }
        }
        3 => {
            if e.tactic_program.time <= 0 && e.turn_program.op == 0 {
                e.tactic_program.arg -= 1;
                if e.tactic_program.arg <= 0 {
                    e.turn_program.start(ops::TURN_TO, 0);
                    e.tactic_program.step += 1;
                } else {
                    e.tactic_program.step = 1;
                }
            }
        }
        4
            if e.turn_program.op == 0 => {
                return true;
            }
        _ => {}
    }
    false
}

/// CEnmParam::TacticStream (0x4167a0), tactic 4: fires the streams of both
/// sides while dashing about.
fn tactic_stream(cx: &mut Cx) -> bool {
    let e = &mut cx.enm;
    let mut step = e.tactic_program.step;
    if step == 0 {
        if e.core_power(0, 6) <= 200 && e.core_power(1, 6) <= 200 {
            return true;
        }
        e.fire_programs[0].start(ops::FIRE_BOOSTED_WAIT, 6);
        e.fire_programs[1].start(ops::FIRE_BOOSTED_WAIT, 6);
        e.move_program.start(ops::MOVE_RANDOM, 0);
        e.tactic_program.step += 1;
        step = 1;
    }
    match step {
        1 => {
            if e.move_program.op == 0 {
                if e.fire_programs[0].op == 0 && e.fire_programs[1].op == 0 {
                    return true;
                }
                e.move_program.start(ops::MOVE_RANDOM, 0);
            }
            if e.core_power(0, 6) <= 0 && e.core_power(1, 6) <= 0 {
                e.fire_programs[0].start(ops::FIRE_ANY, 100);
                e.fire_programs[1].start(ops::FIRE_ANY, 100);
                e.tactic_program.step += 1;
            }
        }
        2
            if e.move_program.op == 0 => {
                return true;
            }
        _ => {}
    }
    false
}

/// CEnmParam::TacticRapidFire (0x4168e0), tactic 5: fires at twice the rate
/// while dashing about for 5 to 15 seconds.
fn tactic_rapid_fire(cx: &mut Cx) -> bool {
    let mut step = cx.enm.tactic_program.step;
    if step == 0 {
        both(cx, ops::FIRE_ANY, 200);
        cx.enm.tactic_program.time = (cx.rng.rand() % 10 + 5) * 60;
        cx.enm.move_program.start(ops::MOVE_RANDOM, 0);
        cx.enm.tactic_program.step += 1;
        step = 1;
    }
    let e = &mut cx.enm;
    if step == 1 && e.move_program.op == 0 {
        if e.core_count < 5 || e.tactic_program.time <= 0 {
            return true;
        }
        e.move_program.start(ops::MOVE_RANDOM, 0);
    }
    false
}

/// CEnmParam::TacticSpinDash (0x4169c0), tactic 7: spins for 20 to 40 seconds
/// while dashing about.
fn tactic_spin_dash(cx: &mut Cx) -> bool {
    match cx.enm.tactic_program.step {
        0 => {
            both(cx, ops::FIRE_ANY, 70);
            let arg = (cx.rng.rand() % 20 + 20) * 60;
            cx.enm.turn_program.start(ops::TURN_SPIN, arg);
            cx.enm.move_program.start(ops::MOVE_RANDOM, 0);
            cx.enm.tactic_program.step += 1;
        }
        1 => {
            let e = &mut cx.enm;
            if e.move_program.op == 0 && e.turn_program.op == 0 {
                return true;
            }
            if e.move_program.op == 0 {
                e.move_program.start(ops::MOVE_RANDOM, 0);
            }
        }
        _ => {}
    }
    false
}

/// CEnmParam::TacticChargeRearSparks (0x416ac0), tactic 9: the opening, then
/// tactic 3.
fn tactic_charge_rear_sparks(cx: &mut Cx) -> bool {
    run_opening(cx, 10);
    let mut step = cx.enm.tactic_program.step;
    if step == 10 {
        cx.enm.fire_programs[0].start(ops::FIRE_ANY, 100);
        cx.enm.fire_programs[1].op = 0;
        cx.enm.tactic_program.arg = cx.rng.rand() % 3 + 3;
        step = 11;
    }
    if step == 20 {
        both(cx, ops::FIRE_ANY, 100);
        let e = &mut cx.enm;
        e.move_program.start(ops::MOVE_RANDOM, 0);
        e.turn_program.start(ops::TURN_TO, 0);
        e.tactic_program.step += 1;
        step = 21;
    }
    let e = &mut cx.enm;
    match step {
        11 => {
            if e.tactic_program.arg > 0 && e.core_power(1, 4) > 0 {
                e.turn_program.start(ops::TURN_FACE, 0x200);
                e.tactic_program.step += 1;
            } else {
                e.tactic_program.step = 20;
            }
        }
        12 => {
            if e.turn_program.op == 0 || e.state.flags & state_flags::TURNED != 0 {
                e.fire_programs[1].start(ops::FIRE_BOOSTED, 4);
                e.tactic_program.wait(60);
            }
        }
        13 => {
            if e.tactic_program.time <= 0 && e.turn_program.op == 0 {
                e.tactic_program.arg -= 1;
                e.tactic_program.step = 11;
            }
        }
        21
            if e.move_program.op == 0 && e.turn_program.op == 0 => {
                return true;
            }
        _ => {}
    }
    false
}

/// CEnmParam::TacticChargeFlip (0x416cb0), tactic 10: the opening, then half a
/// turn at twice the fire rate.
fn tactic_charge_flip(cx: &mut Cx) -> bool {
    run_opening(cx, 10);
    let mut step = cx.enm.tactic_program.step;
    if step == 10 {
        both(cx, ops::FIRE_ANY, 200);
        cx.enm.turn_program.start(ops::TURN_FLIP, 0);
        cx.enm.tactic_program.step += 1;
        step = 11;
    }
    if step == 20 {
        both(cx, ops::FIRE_ANY, 100);
        let e = &mut cx.enm;
        e.move_program.start(ops::MOVE_RANDOM, 0);
        e.turn_program.start(ops::TURN_TO, 0);
        e.tactic_program.step += 1;
        step = 21;
    }
    let e = &mut cx.enm;
    match step {
        11 => {
            if e.turn_program.op == 0 {
                e.tactic_program.step = 20;
            }
        }
        21
            if e.move_program.op == 0 && e.turn_program.op == 0 => {
                return true;
            }
        _ => {}
    }
    false
}

/// CEnmParam::TacticRepeatCharge (0x416e10), tactic 11: faces the player and
/// charges, 3 to 5 times.
fn tactic_repeat_charge(cx: &mut Cx) -> bool {
    let mut step = cx.enm.tactic_program.step;
    if step == 0 {
        cx.enm.tactic_program.arg = cx.rng.rand() % 3 + 3;
        both(cx, ops::FIRE_ANY, 100);
        cx.enm.turn_program.start(ops::TURN_FACE, 0);
        cx.enm.tactic_program.step += 1;
        step = 1;
    }
    let e = &mut cx.enm;
    match step {
        1 => {
            if e.turn_program.op == 0 || e.state.flags & state_flags::TURNED != 0 {
                e.fire_programs[0].op = 0;
                e.fire_programs[1].op = 0;
                e.move_program.start(ops::MOVE_CHARGE, 0);
                e.tactic_program.step += 1;
            }
        }
        2 => {
            if e.move_program.op == 0 && e.turn_program.op == 0 {
                e.tactic_program.wait(60);
            }
        }
        3 => {
            if e.tactic_program.time <= 0 {
                e.tactic_program.arg -= 1;
                e.fire_programs[0].start(ops::FIRE_ANY, 100);
                e.fire_programs[1].start(ops::FIRE_ANY, 100);
                if e.tactic_program.arg <= 0 {
                    e.move_program.start(ops::MOVE_RANDOM, 0);
                    e.turn_program.start(ops::TURN_TO, 0);
                    e.tactic_program.step += 1;
                } else {
                    e.turn_program.start(ops::TURN_FACE, 0);
                    e.tactic_program.step = 1;
                }
            }
        }
        4
            if e.move_program.op == 0 && e.turn_program.op == 0 => {
                return true;
            }
        _ => {}
    }
    false
}

/// CEnmParam::TacticSpin (0x417010), tactic 14: spins for 20 to 40 seconds at
/// twice the fire rate.
fn tactic_spin(cx: &mut Cx) -> bool {
    match cx.enm.tactic_program.step {
        0 => {
            both(cx, ops::FIRE_ANY, 200);
            let arg = (cx.rng.rand() % 20 + 20) * 60;
            cx.enm.turn_program.start(ops::TURN_SPIN, arg);
            cx.enm.tactic_program.step += 1;
        }
        1
            if cx.enm.move_program.op == 0 && cx.enm.turn_program.op == 0 => {
                return true;
            }
        _ => {}
    }
    false
}

/// CEnmParam::TacticLastStand (0x4170c0): tactic 18, once the body has
/// lost its parts: turns back and dashes for good (never over).
fn tactic_last_stand(cx: &mut Cx) -> bool {
    let mut step = cx.enm.tactic_program.step;
    if step == 0 {
        both(cx, ops::FIRE_ANY, 100);
        step = 1;
    }
    let e = &mut cx.enm;
    if step == 1 && e.turn_program.op == 0 {
        e.turn_program.start(ops::TURN_TO, 0);
        e.tactic_program.step += 1;
    }
    if e.move_program.op == 0 {
        e.move_program.start(ops::MOVE_RANDOM, 300);
    }
    false
}

/// The shared body of TacticGlowArmor (0x417430) and TacticGlowRapidFire
/// (0x4176f0).
fn glow_and_dash(cx: &mut Cx, boss: Aid, arg: i32, need_armor: bool) -> bool {
    let mut step = cx.enm.tactic_program.step;
    if step == 0 {
        if need_armor {
            let time = (cx.rng.rand() % 5 + 5) * 60;
            if !glow_armor(cx, boss, time) {
                return true;
            }
            both(cx, ops::FIRE_ANY, arg);
            cx.enm.tactic_program.time = time;
        } else {
            both(cx, ops::FIRE_ANY, arg);
            let time = (cx.rng.rand() % 10 + 5) * 60;
            glow_armor(cx, boss, time);
            cx.enm.tactic_program.time = time;
        }
        cx.enm.move_program.start(ops::MOVE_RANDOM, 0);
        cx.enm.tactic_program.step += 1;
        step = 1;
    }
    let e = &mut cx.enm;
    if step == 1 {
        if e.tactic_program.time <= 0 {
            if e.move_program.op == 0 {
                return true;
            }
        } else if e.move_program.op == 0 {
            e.move_program.start(ops::MOVE_RANDOM, 0);
        }
    }
    false
}

/// CEnmParam::TacticGlowArmor (0x417430), tactic 6: the armored parts glow for
/// 5 to 10 seconds while it dashes about.
fn tactic_glow_armor(cx: &mut Cx, boss: Aid) -> bool {
    glow_and_dash(cx, boss, 70, true)
}

/// CEnmParam::TacticGlowRapidFire (0x4176f0), tactic 16: glows and fires at
/// twice the rate while dashing about.
fn tactic_glow_rapid_fire(cx: &mut Cx, boss: Aid) -> bool {
    glow_and_dash(cx, boss, 200, false)
}

/// CEnmParam::TacticTransform (0x417520), tactic 8: transforms the parts and
/// waits until they are done.
fn tactic_transform(cx: &mut Cx, boss: Aid) -> bool {
    match cx.enm.tactic_program.step {
        2 => {
            if cx.enm.tactic_program.time <= 0 {
                return true;
            }
        }
        1 => {
            if cx.enm.trans_count <= 0 {
                cx.enm.tactic_program.wait(90);
            }
        }
        0 => {
            if !start_transform(cx, boss) {
                return true;
            }
            both(cx, ops::FIRE_ANY, 70);
            cx.enm.tactic_program.step += 1;
        }
        _ => {}
    }
    false
}

/// CEnmParam::TacticSpinGlowSparks (0x4175d0), tactic 15: glows and spins; side
/// 1 fires its sparks.
fn tactic_spin_glow_sparks(cx: &mut Cx, boss: Aid) -> bool {
    match cx.enm.tactic_program.step {
        0 => {
            cx.enm.fire_programs[0].start(ops::FIRE_ANY, 100);
            cx.enm.fire_programs[1].op = 0;
            let time = (cx.rng.rand() % 20 + 20) * 60;
            glow_armor(cx, boss, time);
            cx.enm.turn_program.start(ops::TURN_SPIN, time);
            cx.enm.tactic_program.wait(60);
        }
        1 => {
            let e = &mut cx.enm;
            if e.turn_program.op == 0 {
                return true;
            }
            if e.tactic_program.time <= 0 {
                if e.core_power(1, 4) > 0 {
                    e.fire_programs[1].start(ops::FIRE_BOOSTED, 4);
                } else {
                    e.fire_programs[1].start(ops::FIRE_ANY, 100);
                }
                e.tactic_program.time = 60;
            }
        }
        _ => {}
    }
    false
}

/// CEnmParam::TacticSpinRam (0x4177e0), tactic 17: glows and spins while
/// ramming the player.
fn tactic_spin_ram(cx: &mut Cx, boss: Aid) -> bool {
    match cx.enm.tactic_program.step {
        0 => {
            both(cx, ops::FIRE_ANY, 100);
            let time = (cx.rng.rand() % 20 + 20) * 60;
            glow_armor(cx, boss, time);
            cx.enm.turn_program.start(ops::TURN_SPIN, time);
            cx.enm.move_program.start(ops::MOVE_RAM, 0);
            cx.enm.tactic_program.step += 1;
        }
        1 => {
            let e = &mut cx.enm;
            if e.move_program.op == 0 && e.turn_program.op == 0 {
                return true;
            }
            if e.move_program.op == 0 {
                e.move_program.start(ops::MOVE_RAM, 0);
            }
        }
        _ => {}
    }
    false
}
