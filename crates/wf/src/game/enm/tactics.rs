//! The battle tactics run by list_218_ (enm_tactics.cpp,
//! enm_tactic_helpers.cpp RunOpening). Each returns true when it is over.

use super::battle::{glow_armor, is_side_beaten, signal_unknown_104};
use crate::game::Cx;
use crate::game::actor::Aid;

/// Dispatch of CEnmParam::RunBattle's switch on list_218_.op.
pub fn run(cx: &mut Cx, boss: Aid) -> bool {
    match cx.enm.list_218.op {
        0 => t4161d0(cx),
        1 => t4162f0(cx),
        2 => t416560(cx),
        3 => t416630(cx),
        4 => t4167a0(cx),
        5 => t4168e0(cx),
        6 => t417430(cx, boss),
        7 => t4169c0(cx),
        8 => t417520(cx, boss),
        18 => t4170c0(cx),
        9 => t416ac0(cx),
        10 => t416cb0(cx),
        11 => t416e10(cx),
        12 | 13 => true,
        14 => t417010(cx),
        15 => t4175d0(cx, boss),
        16 => t4176f0(cx, boss),
        17 => t4177e0(cx, boss),
        _ => false,
    }
}

fn both(cx: &mut Cx, op: i32, arg: i32) {
    cx.enm.lists_c4[0].start(op, arg);
    cx.enm.lists_c4[1].start(op, arg);
}

/// 0x4161d0
fn t4161d0(cx: &mut Cx) -> bool {
    match cx.enm.list_218.step {
        0 => {
            both(cx, 0x30001, 100);
            cx.enm.list_218.arg = if cx.enm.state.step_count > 5 { 1 } else { cx.rng.rand() % 3 + 1 };
            cx.enm.list_9c.start(0x10002, 0);
            cx.enm.list_218.step += 1;
        }
        1 => {
            if cx.enm.list_9c.op == 0 {
                if is_side_beaten(cx, 0) {
                    return true;
                }
                cx.enm.list_218.arg -= 1;
                if cx.enm.list_218.arg <= 0 {
                    return true;
                }
                if cx.rng.rand() & 7 != 0 {
                    cx.enm.list_9c.start(0x10002, 0);
                } else {
                    cx.enm.list_9c.start(0x10005, 0);
                }
            }
        }
        _ => {}
    }
    false
}

/// 0x4162f0
fn t4162f0(cx: &mut Cx) -> bool {
    match cx.enm.list_218.step {
        0 => {
            both(cx, 0x30001, 70);
            cx.enm.list_9c.start(0x10002, 0);
            if is_side_beaten(cx, 50) {
                cx.enm.list_b0.start(0x20001, 0);
            }
            cx.enm.list_218.step += 1;
        }
        1 => {
            let e = &mut cx.enm;
            if e.list_9c.op == 0 && e.list_b0.op == 0 {
                return true;
            }
            if e.list_9c.op == 0 {
                e.list_9c.start(0x10002, 0);
            }
        }
        _ => {}
    }
    false
}

/// CEnmParam::RunOpening (0x4163f0): steps 0..3, then `next_step`.
fn run_opening(cx: &mut Cx, next_step: i32) {
    let e = &mut cx.enm;
    if e.list_218.step >= next_step {
        return;
    }
    let mut step = e.list_218.step;
    if step == 0 {
        e.lists_c4[0].start(0x30001, 70);
        e.lists_c4[1].start(0x30001, 70);
        e.list_218.arg = 1;
        e.list_b0.start(0x20004, 0);
        e.list_218.step += 1;
        step = 1;
    }
    match step {
        1 => {
            if e.list_b0.op != 0 && e.state.flags & 2 == 0 {
                return;
            }
            if e.player_dir == 2 && e.list_218.arg > 0 {
                if e.list_b0.op == 0 {
                    e.list_218.arg -= 1;
                    e.list_b0.start(0x20004, 0);
                }
                return;
            }
            e.lists_c4[0].op = 0;
            e.lists_c4[1].op = 0;
            e.list_9c.start(0x10003, 0);
            e.list_218.step += 1;
        }
        2 => {
            if e.list_9c.op == 0 && e.list_b0.op == 0 {
                e.list_218.wait(60);
            }
        }
        3 => {
            if e.list_218.time <= 0 {
                e.list_218.step = next_step;
            }
        }
        _ => {}
    }
}

/// 0x416560
fn t416560(cx: &mut Cx) -> bool {
    run_opening(cx, 10);
    let e = &mut cx.enm;
    match e.list_218.step {
        10 => {
            e.lists_c4[0].start(0x30001, 100);
            e.lists_c4[1].start(0x30001, 100);
            e.list_9c.start(0x10002, 0);
            e.list_b0.start(0x20002, 0);
            e.list_218.step += 1;
        }
        11 => {
            if e.list_9c.op == 0 && e.list_b0.op == 0 {
                return true;
            }
        }
        _ => {}
    }
    false
}

/// 0x416630
fn t416630(cx: &mut Cx) -> bool {
    let mut step = cx.enm.list_218.step;
    if step == 0 {
        cx.enm.lists_c4[0].start(0x30001, 50);
        cx.enm.lists_c4[1].op = 0;
        let arg = cx.rng.rand() % 3 + 3;
        cx.enm.list_218.set_arg_and_next(arg);
        step = 1;
    }
    let e = &mut cx.enm;
    match step {
        1 => {
            if e.core_power(1, 4) == 0 {
                return true;
            }
            e.list_b0.start(0x20004, 0x200);
            e.list_218.step += 1;
        }
        2 => {
            if e.list_b0.op == 0 || e.state.flags & 2 != 0 {
                e.lists_c4[1].start(0x30004, 4);
                e.list_218.wait(60);
            }
        }
        3 => {
            if e.list_218.time <= 0 && e.list_b0.op == 0 {
                e.list_218.arg -= 1;
                if e.list_218.arg <= 0 {
                    e.list_b0.start(0x20002, 0);
                    e.list_218.step += 1;
                } else {
                    e.list_218.step = 1;
                }
            }
        }
        4 => {
            if e.list_b0.op == 0 {
                return true;
            }
        }
        _ => {}
    }
    false
}

/// 0x4167a0
fn t4167a0(cx: &mut Cx) -> bool {
    let e = &mut cx.enm;
    let mut step = e.list_218.step;
    if step == 0 {
        if e.core_power(0, 6) <= 200 && e.core_power(1, 6) <= 200 {
            return true;
        }
        e.lists_c4[0].start(0x30005, 6);
        e.lists_c4[1].start(0x30005, 6);
        e.list_9c.start(0x10002, 0);
        e.list_218.step += 1;
        step = 1;
    }
    match step {
        1 => {
            if e.list_9c.op == 0 {
                if e.lists_c4[0].op == 0 && e.lists_c4[1].op == 0 {
                    return true;
                }
                e.list_9c.start(0x10002, 0);
            }
            if e.core_power(0, 6) <= 0 && e.core_power(1, 6) <= 0 {
                e.lists_c4[0].start(0x30001, 100);
                e.lists_c4[1].start(0x30001, 100);
                e.list_218.step += 1;
            }
        }
        2 => {
            if e.list_9c.op == 0 {
                return true;
            }
        }
        _ => {}
    }
    false
}

/// 0x4168e0
fn t4168e0(cx: &mut Cx) -> bool {
    let mut step = cx.enm.list_218.step;
    if step == 0 {
        both(cx, 0x30001, 200);
        cx.enm.list_218.time = (cx.rng.rand() % 10 + 5) * 60;
        cx.enm.list_9c.start(0x10002, 0);
        cx.enm.list_218.step += 1;
        step = 1;
    }
    let e = &mut cx.enm;
    if step == 1 && e.list_9c.op == 0 {
        if e.core_count < 5 || e.list_218.time <= 0 {
            return true;
        }
        e.list_9c.start(0x10002, 0);
    }
    false
}

/// 0x4169c0
fn t4169c0(cx: &mut Cx) -> bool {
    match cx.enm.list_218.step {
        0 => {
            both(cx, 0x30001, 70);
            let arg = (cx.rng.rand() % 20 + 20) * 60;
            cx.enm.list_b0.start(0x20003, arg);
            cx.enm.list_9c.start(0x10002, 0);
            cx.enm.list_218.step += 1;
        }
        1 => {
            let e = &mut cx.enm;
            if e.list_9c.op == 0 && e.list_b0.op == 0 {
                return true;
            }
            if e.list_9c.op == 0 {
                e.list_9c.start(0x10002, 0);
            }
        }
        _ => {}
    }
    false
}

/// 0x416ac0
fn t416ac0(cx: &mut Cx) -> bool {
    run_opening(cx, 10);
    let mut step = cx.enm.list_218.step;
    if step == 10 {
        cx.enm.lists_c4[0].start(0x30001, 100);
        cx.enm.lists_c4[1].op = 0;
        cx.enm.list_218.arg = cx.rng.rand() % 3 + 3;
        step = 11;
    }
    if step == 20 {
        both(cx, 0x30001, 100);
        let e = &mut cx.enm;
        e.list_9c.start(0x10002, 0);
        e.list_b0.start(0x20002, 0);
        e.list_218.step += 1;
        step = 21;
    }
    let e = &mut cx.enm;
    match step {
        11 => {
            if e.list_218.arg > 0 && e.core_power(1, 4) > 0 {
                e.list_b0.start(0x20004, 0x200);
                e.list_218.step += 1;
            } else {
                e.list_218.step = 20;
            }
        }
        12 => {
            if e.list_b0.op == 0 || e.state.flags & 2 != 0 {
                e.lists_c4[1].start(0x30004, 4);
                e.list_218.wait(60);
            }
        }
        13 => {
            if e.list_218.time <= 0 && e.list_b0.op == 0 {
                e.list_218.arg -= 1;
                e.list_218.step = 11;
            }
        }
        21 => {
            if e.list_9c.op == 0 && e.list_b0.op == 0 {
                return true;
            }
        }
        _ => {}
    }
    false
}

/// 0x416cb0
fn t416cb0(cx: &mut Cx) -> bool {
    run_opening(cx, 10);
    let mut step = cx.enm.list_218.step;
    if step == 10 {
        both(cx, 0x30001, 200);
        cx.enm.list_b0.start(0x20001, 0);
        cx.enm.list_218.step += 1;
        step = 11;
    }
    if step == 20 {
        both(cx, 0x30001, 100);
        let e = &mut cx.enm;
        e.list_9c.start(0x10002, 0);
        e.list_b0.start(0x20002, 0);
        e.list_218.step += 1;
        step = 21;
    }
    let e = &mut cx.enm;
    match step {
        11 => {
            if e.list_b0.op == 0 {
                e.list_218.step = 20;
            }
        }
        21 => {
            if e.list_9c.op == 0 && e.list_b0.op == 0 {
                return true;
            }
        }
        _ => {}
    }
    false
}

/// 0x416e10
fn t416e10(cx: &mut Cx) -> bool {
    let mut step = cx.enm.list_218.step;
    if step == 0 {
        cx.enm.list_218.arg = cx.rng.rand() % 3 + 3;
        both(cx, 0x30001, 100);
        cx.enm.list_b0.start(0x20004, 0);
        cx.enm.list_218.step += 1;
        step = 1;
    }
    let e = &mut cx.enm;
    match step {
        1 => {
            if e.list_b0.op == 0 || e.state.flags & 2 != 0 {
                e.lists_c4[0].op = 0;
                e.lists_c4[1].op = 0;
                e.list_9c.start(0x10003, 0);
                e.list_218.step += 1;
            }
        }
        2 => {
            if e.list_9c.op == 0 && e.list_b0.op == 0 {
                e.list_218.wait(60);
            }
        }
        3 => {
            if e.list_218.time <= 0 {
                e.list_218.arg -= 1;
                e.lists_c4[0].start(0x30001, 100);
                e.lists_c4[1].start(0x30001, 100);
                if e.list_218.arg <= 0 {
                    e.list_9c.start(0x10002, 0);
                    e.list_b0.start(0x20002, 0);
                    e.list_218.step += 1;
                } else {
                    e.list_b0.start(0x20004, 0);
                    e.list_218.step = 1;
                }
            }
        }
        4 => {
            if e.list_9c.op == 0 && e.list_b0.op == 0 {
                return true;
            }
        }
        _ => {}
    }
    false
}

/// 0x417010
fn t417010(cx: &mut Cx) -> bool {
    match cx.enm.list_218.step {
        0 => {
            both(cx, 0x30001, 200);
            let arg = (cx.rng.rand() % 20 + 20) * 60;
            cx.enm.list_b0.start(0x20003, arg);
            cx.enm.list_218.step += 1;
        }
        1 => {
            if cx.enm.list_9c.op == 0 && cx.enm.list_b0.op == 0 {
                return true;
            }
        }
        _ => {}
    }
    false
}

/// 0x4170c0: the last stand (never over).
fn t4170c0(cx: &mut Cx) -> bool {
    let mut step = cx.enm.list_218.step;
    if step == 0 {
        both(cx, 0x30001, 100);
        step = 1;
    }
    let e = &mut cx.enm;
    if step == 1 && e.list_b0.op == 0 {
        e.list_b0.start(0x20002, 0);
        e.list_218.step += 1;
    }
    if e.list_9c.op == 0 {
        e.list_9c.start(0x10002, 300);
    }
    false
}

/// The shared body of 0x417430 and 0x4176f0.
fn glow_and_dash(cx: &mut Cx, boss: Aid, arg: i32, need_armor: bool) -> bool {
    let mut step = cx.enm.list_218.step;
    if step == 0 {
        if need_armor {
            let time = (cx.rng.rand() % 5 + 5) * 60;
            if !glow_armor(cx, boss, time) {
                return true;
            }
            both(cx, 0x30001, arg);
            cx.enm.list_218.time = time;
        } else {
            both(cx, 0x30001, arg);
            let time = (cx.rng.rand() % 10 + 5) * 60;
            glow_armor(cx, boss, time);
            cx.enm.list_218.time = time;
        }
        cx.enm.list_9c.start(0x10002, 0);
        cx.enm.list_218.step += 1;
        step = 1;
    }
    let e = &mut cx.enm;
    if step == 1 {
        if e.list_218.time <= 0 {
            if e.list_9c.op == 0 {
                return true;
            }
        } else if e.list_9c.op == 0 {
            e.list_9c.start(0x10002, 0);
        }
    }
    false
}

/// 0x417430
fn t417430(cx: &mut Cx, boss: Aid) -> bool {
    glow_and_dash(cx, boss, 70, true)
}

/// 0x4176f0
fn t4176f0(cx: &mut Cx, boss: Aid) -> bool {
    glow_and_dash(cx, boss, 200, false)
}

/// 0x417520
fn t417520(cx: &mut Cx, boss: Aid) -> bool {
    match cx.enm.list_218.step {
        2 => {
            if cx.enm.list_218.time <= 0 {
                return true;
            }
        }
        1 => {
            if cx.enm.trans_count <= 0 {
                cx.enm.list_218.wait(90);
            }
        }
        0 => {
            if !signal_unknown_104(cx, boss) {
                return true;
            }
            both(cx, 0x30001, 70);
            cx.enm.list_218.step += 1;
        }
        _ => {}
    }
    false
}

/// 0x4175d0
fn t4175d0(cx: &mut Cx, boss: Aid) -> bool {
    match cx.enm.list_218.step {
        0 => {
            cx.enm.lists_c4[0].start(0x30001, 100);
            cx.enm.lists_c4[1].op = 0;
            let time = (cx.rng.rand() % 20 + 20) * 60;
            glow_armor(cx, boss, time);
            cx.enm.list_b0.start(0x20003, time);
            cx.enm.list_218.wait(60);
        }
        1 => {
            let e = &mut cx.enm;
            if e.list_b0.op == 0 {
                return true;
            }
            if e.list_218.time <= 0 {
                if e.core_power(1, 4) > 0 {
                    e.lists_c4[1].start(0x30004, 4);
                } else {
                    e.lists_c4[1].start(0x30001, 100);
                }
                e.list_218.time = 60;
            }
        }
        _ => {}
    }
    false
}

/// 0x4177e0
fn t4177e0(cx: &mut Cx, boss: Aid) -> bool {
    match cx.enm.list_218.step {
        0 => {
            both(cx, 0x30001, 100);
            let time = (cx.rng.rand() % 20 + 20) * 60;
            glow_armor(cx, boss, time);
            cx.enm.list_b0.start(0x20003, time);
            cx.enm.list_9c.start(0x10005, 0);
            cx.enm.list_218.step += 1;
        }
        1 => {
            let e = &mut cx.enm;
            if e.list_9c.op == 0 && e.list_b0.op == 0 {
                return true;
            }
            if e.list_9c.op == 0 {
                e.list_9c.start(0x10005, 0);
            }
        }
        _ => {}
    }
    false
}
