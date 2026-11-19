//! Effects and explosions: control 8 (an EffectDesc sprite), 16..20 (the
//! explosions of the player and the boss), 22 (the glow of a part) and 23
//! (the flare of a part). effect_controls.cpp, explosion_controls.cpp,
//! actor_control.cpp.

use super::{ActorDesc, AnmData, Aid, Obj, Param, create, destroy, flags, sheet, spawn};
use crate::game::Cx;
use crate::game::math::{cos, sin};
use crate::game::sounds;
use bytemuck::{Pod, Zeroable};

/// g_explosion_anim (0x45d37c): sys0 16..21 (its set is g_pats.sys, 0x467d70),
/// 3 frames each.
pub static EXPLOSION_ANIM: AnmData = AnmData { set: sheet::SYS, script: &[16, 3, 17, 3, 18, 3, 19, 3, 20, 3, 21, 3, -1] };
/// g_fragment_anim (0x45d3a4): sys0 22..28 (set 0x467d70).
pub static FRAGMENT_ANIM: AnmData = AnmData { set: sheet::SYS, script: &[22, 3, 23, 3, 24, 3, 25, 3, 26, 3, 27, 3, 28, 3, -1] };

/// Actor::work of control 8.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EffectWork {
    fading: i32,
    life: i32,
    grow: f32,
    color: [i16; 4],
    fade: [i16; 4],
}

fn argb(c: [i16; 4]) -> u32 {
    ((((((c[3] as i32) << 8) | c[0] as i32) << 8 | c[1] as i32) << 8) | c[2] as i32) as u32
}

/// Control08::OnCreate (0x412cb0).
pub fn effect_create(cx: &mut Cx, a: Aid, param: Param) -> bool {
    let desc = match param {
        Param::Effect(d) => d,
        _ => cx.ctl.default_effect,
    };
    cx.actors.set_sprite(a, desc.sprite);
    let start_angle = if desc.angle_min == desc.angle_max {
        desc.angle_min as u16
    } else {
        (desc.angle_min as i32 + cx.rng.rand() % (desc.angle_max as i32 - desc.angle_min as i32)) as u16
    };
    let speed = if desc.speed_min == desc.speed_max {
        desc.speed_min
    } else {
        ((cx.rng.rand() & 0x3ff) as f32 / 1024.0) * (desc.speed_max - desc.speed_min) + desc.speed_min
    };
    let scale = if desc.scale_min == desc.scale_max {
        desc.scale_min
    } else {
        ((cx.rng.rand() & 0x3ff) as f32 / 1024.0) * (desc.scale_max - desc.scale_min) + desc.scale_min
    };
    let x = cx.actors.get_mut(a);
    x.hit_points = 1;
    x.flags |= desc.flags as u32;
    x.team = 0;
    x.acceleration = desc.acceleration;
    x.start_angle = start_angle;
    x.speed = speed;
    x.scale = scale;
    x.matrix_angle = x.matrix_angle.wrapping_add(0x1000);
    x.color = argb(desc.color);
    if desc.life == 0 && desc.grow >= 0.0 && desc.fade[3] >= 0 {
        x.flags |= flags::STOP_DIES;
    }
    let fading = desc.fade.iter().any(|&f| f != 0);
    let w = cx.actors.w::<EffectWork>(a);
    if fading {
        w.fading = 1;
        w.color = desc.color;
        w.fade = desc.fade;
    } else {
        w.fading = 0;
    }
    w.life = desc.life as i32;
    w.grow = desc.grow;
    true
}

/// Control08::OnMove (0x412e70).
pub fn effect_move(cx: &mut Cx, a: Aid) -> bool {
    let w = *cx.actors.w::<EffectWork>(a);
    let x = cx.actors.get_mut(a);
    if w.life != 0 && x.age as i32 > w.life {
        return false;
    }
    if w.grow != 0.0 {
        let s = x.scale + w.grow;
        x.set_scale(s);
        if x.scale < 0.0 {
            return false;
        }
    }
    if w.fading != 0 {
        let w = cx.actors.w::<EffectWork>(a);
        for i in 0..4 {
            if w.fade[i] != 0 {
                w.color[i] = w.color[i].wrapping_add(w.fade[i]);
                w.color[i] = w.color[i].clamp(0, 255);
            }
        }
        if w.color[3] == 0 {
            return false;
        }
        let c = argb(w.color);
        cx.actors.get_mut(a).color = c;
    }
    true
}

/// Control16::OnCreate (0x410640): the player's explosion.
pub fn c16_create(cx: &mut Cx, a: Aid) -> bool {
    cx.ctl.explosion_depth += 1;
    let (x, y) = {
        let act = cx.actors.get_mut(a);
        act.hit_points = 1;
        act.team = 0;
        (act.x(), act.y())
    };
    create(cx, 18, None, x, y, 8, 0, Param::None);
    cx.ctl.explosion_depth -= 1;
    cx.ctl.spark_desc.x = x;
    cx.ctl.spark_desc.y = y;
    for i in 0..48 {
        cx.ctl.spark_desc.start_angle = (i * 0x400 / 48) as u16;
        let d = cx.ctl.spark_desc;
        spawn(cx, &d);
    }
    if cx.ctl.explosion_depth == 0 {
        cx.play_sound(sounds::BOMB_ME);
    }
    true
}

/// Control17::OnCreate (0x410930): a small explosion.
pub fn c17_create(cx: &mut Cx, a: Aid) -> bool {
    let anim = cx.actors.play(&EXPLOSION_ANIM);
    cx.actors.set_sprite(a, anim);
    let (x, y) = {
        let act = cx.actors.get_mut(a);
        act.hit_points = 1;
        act.team = 0;
        act.start_angle = 0x200;
        act.speed = 8.0;
        (act.x(), act.y())
    };
    cx.actors.set_layer(a, 9);
    let count = if cx.actors.count >= 700 { 2 } else { 8 };
    for _ in 0..count {
        let e = cx.ctl.smoke_effect;
        create(cx, 8, None, x, y, 9, 0, Param::Effect(e));
    }
    if cx.ctl.explosion_depth == 0 {
        cx.play_sound(sounds::BOMB1);
    }
    true
}

/// Two rand() offsets of an explosion's position; MSVC evaluates the
/// arguments of Create right to left, so y's comes first.
fn scatter(cx: &mut Cx, x: f32, y: f32, mx: i32, ox: i32, my: i32, oy: i32) -> (f32, f32) {
    let ry = (cx.rng.rand() & my) - oy;
    let rx = (cx.rng.rand() & mx) - ox;
    (x + rx as f32, y + ry as f32)
}

/// Control18::OnCreate (0x410a00): a medium explosion.
pub fn c18_create(cx: &mut Cx, a: Aid) -> bool {
    let (x, y) = {
        let act = cx.actors.get_mut(a);
        act.hit_points = 1;
        act.team = 0;
        (act.x(), act.y())
    };
    cx.actors.set_layer(a, 8);
    cx.ctl.explosion_depth += 1;
    create(cx, 17, None, x, y, 9, 0, Param::None);
    cx.ctl.explosion_depth -= 1;
    if cx.ctl.explosion_depth == 0 {
        cx.play_sound(sounds::BOMB2);
    }
    true
}

/// Control18::OnMove (0x410a80).
pub fn c18_move(cx: &mut Cx, a: Aid) -> bool {
    let (age, x, y) = {
        let act = cx.actors.get(a);
        (act.age, act.x(), act.y())
    };
    match age {
        4 | 8 | 12 => {
            cx.ctl.explosion_depth += 1;
            let (px, py) = scatter(cx, x, y, 0x7f, 64, 0x7f, 64);
            create(cx, 17, None, px, py, 9, 0, Param::None);
            cx.ctl.explosion_depth -= 1;
        }
        13 => return false,
        _ => {}
    }
    if age < 6 {
        cx.ctl.fragment_desc.x = x;
        cx.ctl.fragment_desc.y = y;
        cx.ctl.fragment_desc.layer = 8;
        cx.ctl.fragment_desc.acceleration = -0.1;
        let count = if cx.actors.count >= 700 { 1 } else { 4 };
        for _ in 0..count {
            cx.ctl.fragment_desc.start_angle = (cx.rng.rand() & 0x3ff) as u16;
            cx.ctl.fragment_desc.sprite = cx.actors.play(&FRAGMENT_ANIM);
            cx.ctl.fragment_desc.speed = ((cx.rng.rand() & 7) + 1) as f32;
            let d = cx.ctl.fragment_desc;
            spawn(cx, &d);
        }
    }
    true
}

/// Actor::work of control 19.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct C19Work {
    pieces: [u32; 3],
    spins: [i16; 3],
    alpha: i16,
}

/// The rings of a shockwave: three triangles turning around the actor.
fn spawn_rings(cx: &mut Cx, a: Aid, min_spin: i32) -> ([u32; 3], [i16; 3]) {
    cx.ctl.shockwave_desc.layer = 7;
    cx.ctl.shockwave_desc.parent = Some(a);
    let mut pieces = [0u32; 3];
    let mut spins = [0i16; 3];
    for i in 0..3 {
        cx.ctl.shockwave_desc.angle = ((cx.rng.rand() & 0x7f) + i as i32 * 300) as u16;
        let d = cx.ctl.shockwave_desc;
        pieces[i] = spawn(cx, &d).map_or(0, |p| p as u32);
        spins[i] = if cx.rng.rand() & 1 != 0 { ((cx.rng.rand() & 7) + min_spin) as i16 } else { (-min_spin - (cx.rng.rand() & 7)) as i16 };
    }
    (pieces, spins)
}

/// Control19::OnCreate (0x410be0): a large explosion.
pub fn c19_create(cx: &mut Cx, a: Aid) -> bool {
    let (x, y) = {
        let act = cx.actors.get_mut(a);
        act.hit_points = 1;
        act.team = 0;
        (act.x(), act.y())
    };
    cx.actors.set_layer(a, 8);
    cx.ctl.spark_desc.x = x;
    cx.ctl.spark_desc.y = y;
    if cx.actors.count < 700 {
        for i in 0..48 {
            cx.ctl.spark_desc.start_angle = (i * 0x400 / 48) as u16;
            let d = cx.ctl.spark_desc;
            spawn(cx, &d);
        }
    }
    let (pieces, spins) = spawn_rings(cx, a, 2);
    let w = cx.actors.w::<C19Work>(a);
    w.pieces = pieces;
    w.spins = spins;
    w.alpha = 0x7e;
    cx.ctl.explosion_depth += 1;
    create(cx, 18, None, x, y, 8, 0, Param::None);
    cx.ctl.explosion_depth -= 1;
    if cx.ctl.explosion_depth == 0 {
        cx.play_sound(sounds::BOMB3);
    }
    true
}

fn grey(alpha: i32) -> u32 {
    ((((alpha as u32 | 0xffff_ff00) << 8) | alpha as u32) << 8) | alpha as u32
}

/// Turns the rings and sets their colour.
fn spin_rings(cx: &mut Cx, rings: [u32; 3], spins: [i16; 3], color: u32) {
    for i in 0..3 {
        let r = cx.actors.get_mut(rings[i] as Aid);
        r.angle = r.angle.wrapping_add(spins[i]);
        r.color = color;
    }
}

/// Control19::OnMove (0x410710).
pub fn c19_move(cx: &mut Cx, a: Aid) -> bool {
    let w = cx.actors.w::<C19Work>(a);
    if w.alpha > 0 {
        w.alpha -= 3;
    }
    let w = *w;
    spin_rings(cx, w.pieces, w.spins, grey(w.alpha as i32));
    let (age, x, y) = {
        let act = cx.actors.get(a);
        (act.age, act.x(), act.y())
    };
    match age {
        4 | 8 | 12 => {
            cx.ctl.explosion_depth += 1;
            let (px, py) = scatter(cx, x, y, 0x7f, 64, 0x7f, 64);
            create(cx, 18, None, px, py, 8, 0, Param::None);
            cx.ctl.explosion_depth -= 1;
        }
        36 => return false,
        _ => {}
    }
    true
}

/// Actor::work of control 20.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct C20Work {
    core: u32,
    rings: [u32; 3],
    spins: [i16; 3],
    alpha: i16,
}

/// Control20::OnCreate (0x410d50): the boss blowing up.
pub fn c20_create(cx: &mut Cx, a: Aid) -> bool {
    let sprite = cx.pats.get(sheet::SYS, 32);
    cx.actors.set_sprite(a, sprite);
    let angle = ((cx.rng.rand() & 0x3f) - 0x20) as i16;
    let start = ((cx.rng.rand() & 0x7f) - 0x40) as u16;
    {
        let act = cx.actors.get_mut(a);
        act.hit_points = 1;
        act.team = 0;
        act.angle = angle;
        act.start_angle = start;
        act.speed = 2.0;
        act.acceleration = -0.02;
    }
    cx.actors.set_layer(a, 4);
    let desc = ActorDesc { color: 0xffc0_6000, hit_points: 1, sprite: cx.pats.get(sheet::ENM, 1), parent: Some(a), layer: 4, ..Default::default() };
    let core = spawn(cx, &desc).map_or(0, |c| c as u32);
    cx.actors.w::<C20Work>(a).core = core;
    if cx.ctl.explosion_depth == 0 {
        cx.play_sound(sounds::BOMB3);
    }
    cx.game.boss = Some(a);
    true
}

/// Control20::OnMove (0x410e70).
pub fn c20_move(cx: &mut Cx, a: Aid) -> bool {
    let (age, x, y) = {
        let act = cx.actors.get(a);
        (act.age, act.x(), act.y())
    };
    match age {
        50 | 80 => {
            let angle = ((cx.rng.rand() & 0x7f) - 64) as i16;
            let start = ((cx.rng.rand() & 0x1ff) + 0x100) as u16;
            let act = cx.actors.get_mut(a);
            act.angle = angle;
            act.start_angle = start;
            act.speed = 2.0;
            act.acceleration = -0.02;
            create(cx, 18, None, x, y, 8, 0, Param::None);
        }
        90 => {
            let (rings, spins) = spawn_rings(cx, a, 1);
            for r in rings {
                cx.actors.get_mut(r as Aid).color = 0;
            }
            let w = cx.actors.w::<C20Work>(a);
            w.rings = rings;
            w.spins = spins;
            w.alpha = 0;
        }
        100 => cx.play_sound(sounds::BOMB_PRE),
        113 => cx.fade().set_color(0xff, 0xff, 0xff, 0),
        110 | 120 => cx.fade().set_color(0xff, 0xff, 0xff, 0x40),
        123 => {
            cx.fade().set_color(0xff, 0xff, 0xff, 0);
            cx.fade().fade_to(28, 0xff, 0xff, 0xff, 0x40);
        }
        150 => {
            let w = *cx.actors.w::<C20Work>(a);
            for r in w.rings {
                destroy(cx, r as Aid, true, true);
            }
            destroy(cx, w.core as Aid, true, true);
            cx.actors.set_sprite(a, Obj::Null);
            cx.ctl.explosion_depth += 1;
            create(cx, 19, None, x, y, 8, 0, Param::None);
            cx.ctl.explosion_depth -= 1;
            cx.fade().set_color(0xff, 0xff, 0xff, 0xff);
            cx.fade().fade_to(120, 0xff, 0xff, 0xff, 0);
            cx.sound.stop_channel(sounds::BOMB_PRE);
            cx.play_sound(sounds::BOMB4);
            cx.play_sound(sounds::BOMB_ALL);
        }
        165 | 180 => {
            cx.ctl.explosion_depth += 1;
            let (px, py) = scatter(cx, x, y, 0x7f, 64, 0x7f, 64);
            create(cx, 19, None, px, py, 8, 0, Param::None);
            cx.ctl.explosion_depth -= 1;
        }
        270 => return false,
        _ => {}
    }
    if age < 150 {
        let core = cx.actors.w::<C20Work>(a).core as Aid;
        cx.actors.get_mut(core).color = if cx.app.frame_count & 2 != 0 { 0xffff_4000 } else { 0xff00_ff00 };
        if cx.rng.rand() & 7 == 0 {
            let (px, py) = scatter(cx, x, y, 0x3f, 31, 0x7f, 64);
            create(cx, 17, None, px, py, 9, 0, Param::None);
        }
        if age > 90 {
            let w = cx.actors.w::<C20Work>(a);
            w.alpha += 2;
            let w = *w;
            spin_rings(cx, w.rings, w.spins, grey(w.alpha as i32));
        }
    }
    true
}

/// Actor::work of control 22.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlowWork {
    timer: i32,
    state: i32,
    duration: i32,
    phase: i32,
    spin: i16,
    _pad: i16,
}

/// Control22::OnCreate (0x410470): `param` is the glow time.
pub fn glow_create(cx: &mut Cx, a: Aid, param: Param) -> bool {
    let sprite = cx.pats.get(sheet::ADD, 27);
    cx.actors.set_sprite(a, sprite);
    let act = cx.actors.get_mut(a);
    act.hit_points = 1;
    act.team = 0;
    act.flags |= flags::ADDITIVE;
    let time = if let Param::Int(t) = param { t } else { 0 };
    let w = cx.actors.w::<GlowWork>(a);
    w.duration = time - 120;
    w.state = 0;
    true
}

/// Control22::OnMove (0x410350).
pub fn glow_move(cx: &mut Cx, a: Aid) -> bool {
    let mut w = *cx.actors.w::<GlowWork>(a);
    let mut alpha = 0.0f32;
    if w.state == 0 {
        w.phase = cx.rng.rand() & 0xff;
        w.spin = if cx.rng.rand() & 1 != 0 { 2 } else { -2 };
        w.timer = 0;
        w.state += 1;
    }
    match w.state {
        1 => {
            alpha = w.timer as f32 * (1.0 / 60.0);
            if w.timer >= 60 {
                w.timer = 0;
                w.state += 1;
            }
        }
        2 => {
            alpha = 1.0;
            if w.timer >= w.duration {
                w.timer = 0;
                w.state = 3;
            }
        }
        3 => {
            alpha = 1.0 - w.timer as f32 * (1.0 / 60.0);
            if w.timer >= 60 {
                *cx.actors.w::<GlowWork>(a) = w;
                return true;
            }
        }
        _ => {}
    }
    let pulse = crate::game::math::sin_table()[((w.phase & 0xff) * 4) as usize] * 0.2 + 0.8;
    let act = cx.actors.get_mut(a);
    act.color = ((crate::game::math::ftol(pulse * alpha * 255.0) as u32) << 24) | 0xff8800;
    act.angle = act.angle.wrapping_add(w.spin);
    w.phase += 1;
    w.timer += 1;
    *cx.actors.w::<GlowWork>(a) = w;
    true
}

/// Actor::work of control 23.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FlareWork {
    max_scale: f32,
    scale: f32,
    timer: i32,
}

/// Control23::OnCreate (0x411380, matched).
pub fn flare_create(cx: &mut Cx, a: Aid) -> bool {
    let (time, size) = (cx.enm.flare_time, cx.enm.flare_size);
    let act = cx.actors.get_mut(a);
    act.hit_points = 1;
    act.team = 0;
    act.angle = act.start_angle as i16;
    let w = cx.actors.w::<FlareWork>(a);
    w.timer = time;
    w.max_scale = ((size - 1.0) * 1.5 + 1.0) * 0.2;
    w.scale = 0.0;
    true
}

/// Control23::OnMove (0x411450).
pub fn flare_move(cx: &mut Cx, a: Aid) -> bool {
    let w = cx.actors.w::<FlareWork>(a);
    w.timer -= 1;
    if w.timer <= 0 {
        return false;
    }
    w.scale += 0.04;
    if w.scale > w.max_scale {
        w.scale = w.max_scale;
    }
    let scale = w.scale;
    if cx.actors.get(a).age & 3 == 1 {
        let angle = cx.actors.screen_angle(a) as i16;
        let act = cx.actors.get(a);
        let (x, y, layer) = (act.sx(), act.sy(), act.layer);
        let mut e = cx.ctl.flare_trail;
        e.sprite = cx.pats.get(sheet::ADD, 28);
        e.scale_min = scale;
        e.scale_max = scale;
        e.angle_min = angle;
        e.angle_max = angle;
        cx.ctl.flare_trail = e;
        create(cx, 8, None, x, y, layer, 0, Param::Effect(e));
    }
    true
}

/// The muzzle offset of a direction (used by the player's laser).
pub fn polar(angle: i32, length: f32) -> (f32, f32) {
    (sin(angle) * length, cos(angle) * -length)
}
