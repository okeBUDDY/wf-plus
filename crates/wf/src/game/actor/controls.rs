//! The actor controls (actor_control.h): one stateless behaviour per id,
//! registered by cGlobal::RegisterActorControls (0x40ff00). Here they are
//! a dispatch on the id; their static data lives in [`ControlData`].

use super::{ActorDesc, Aid, EffectDesc, FloatRect, Obj, Param, boss, cores, fx, parts, player};
use crate::game::Cx;

/// The ids RegisterActorControls registers.
const REGISTERED: [i32; 24] = [1, 8, 16, 17, 18, 19, 20, 21, 22, 23, 35, 43, 32, 33, 34, 40, 41, 42, 44, 47, 46, 49, 45, 48];

/// The controls of the cores by weapon (0x45d91c).
pub const CORE_CONTROLS: [i32; 7] = [40, 41, 42, 44, 46, 47, 49];
/// The debris controls by size (0x45d938).
pub const DEBRIS_CONTROLS: [i16; 4] = [0, 17, 18, 19];

pub fn registered(id: i32) -> bool {
    REGISTERED.contains(&id)
}

/// The player's laser (LaserSprite, g_laser 0x4690f0).
#[derive(Clone, Copy, Debug)]
pub struct LaserSprite {
    /// The fan, relative to the ship.
    pub points: [[f32; 2]; 3],
    pub tip_x: f32,
    pub tip_y: f32,
    pub x: f32,
    pub y: f32,
    pub angle: i16,
    pub spread: i16,
}

impl Default for LaserSprite {
    /// LaserSprite::LaserSprite (0x4153c0).
    fn default() -> Self {
        LaserSprite { points: [[0.0; 2]; 3], tip_x: 0.0, tip_y: -140.0, x: 0.0, y: 0.0, angle: 0, spread: 16 }
    }
}

/// The static data of the controls.
#[derive(Default)]
pub struct ControlData {
    /// Control32::bounds_ (0x469054): where the boss body may move.
    pub boss_bounds: FloatRect,
    /// Control32::boss_ (0x469270): the body while it is alive.
    pub boss: Option<Aid>,
    /// Control32::weapons_ (0x469064).
    pub weapons: [i32; 7],
    /// CorePartControl::max_unknown_22_ (0x469274).
    pub max_unknown_22: i32,
    /// g_laser.
    pub laser: LaserSprite,
    /// Control01::weapon_ (0x4690ec), kept from life to life.
    pub weapon: i32,
    /// Control01::shot_ (0x4690b8).
    pub shot: ActorDesc,
    /// g_muzzle_effect (0x45d4ac).
    pub muzzle: EffectDesc,
    /// Control08::default_ (0x45d458).
    pub default_effect: EffectDesc,
    /// The bullets of controls 35, 40, 41, 46 and 49.
    pub desc35: ActorDesc,
    pub desc40: ActorDesc,
    pub desc41: ActorDesc,
    pub desc46: ActorDesc,
    pub desc49: ActorDesc,
    /// Control43::glow_desc_, beam_desc_.
    pub glow_desc: ActorDesc,
    pub beam_desc: ActorDesc,
    /// Control43::sound_users_ (0x469200).
    pub sound_users: [i32; 2],
    /// g_explosion_depth (0x469048): a file-static counter of
    /// explosion_controls.cpp; only actor/fx.rs (that file's port) uses it.
    pub(super) explosion_depth: i32,
    pub fragment_desc: ActorDesc,
    pub spark_desc: ActorDesc,
    pub shockwave_desc: ActorDesc,
    /// g_smoke_effect (0x45d3ac).
    pub smoke_effect: EffectDesc,
    /// g_flare_trail (0x45d3e0).
    pub flare_trail: EffectDesc,
}

/// The EffectDesc tables of wf.exe's .data, without their sprites (set
/// by the controls' Initialize).
fn effect(flags: u16, life: u16, scale: [f32; 2], grow: f32, speed: [f32; 2], acceleration: f32, angle: [i16; 2], color: [i16; 4], fade: [i16; 4]) -> EffectDesc {
    EffectDesc {
        sprite: Obj::Null,
        flags,
        life,
        scale_min: scale[0],
        scale_max: scale[1],
        grow,
        speed_min: speed[0],
        speed_max: speed[1],
        acceleration,
        angle_min: angle[0],
        angle_max: angle[1],
        color,
        fade,
    }
}

/// cGlobal::RegisterActorControls: the Initialize() of every control.
pub fn register_all(cx: &mut Cx) {
    let mut d = ControlData {
        // 0x45d4ac
        muzzle: effect(16, 0, [0.5, 0.5], -0.03, [6.0, 3.0], -0.2, [312, 712], [255, 255, 255, 255], [0; 4]),
        // 0x45d458
        default_effect: effect(18, 0, [0.5, 0.2], -0.01, [10.0, 6.0], -0.2, [0, 1024], [128, 128, 128, 255], [0; 4]),
        // 0x45d3ac
        smoke_effect: effect(16, 0, [1.2, 0.5], -0.04, [10.0, 4.0], -0.1, [0, 1024], [192, 192, 192, 255], [0; 4]),
        // 0x45d3e0
        flare_trail: effect(16, 0, [1.0, 1.0], 0.1, [1.5, 1.5], 0.0, [0, 0], [255, 255, 255, 255], [0, 0, 0, -10]),
        ..Default::default()
    };
    let pats = &cx.pats;
    use super::sheet::{ADD, SYS};
    // Control01::Initialize (0x412fe0).
    d.shot = ActorDesc { color: 0xffff_ffff, sprite: pats.get(SYS, 2), team: 8, debris: 8, flags: 0x11, hit_points: 1, speed: 16.0, layer: 10, ..Default::default() };
    d.muzzle.sprite = pats.get(ADD, 1);
    // Control08::Initialize (0x412c80).
    d.default_effect.sprite = pats.get(ADD, 1);
    // Control16::Initialize (0x410540).
    d.fragment_desc = ActorDesc { color: 0xffff_ffff, age: 36, flags: super::flags::STOP_DIES, hit_points: 1, ..Default::default() };
    d.spark_desc = ActorDesc { color: 0xffff_ffff, flags: super::flags::FACE_START, hit_points: 1, sprite: pats.get(SYS, 8), layer: 0, speed: 8.0, acceleration: 2.0, ..Default::default() };
    d.shockwave_desc = ActorDesc { flags: super::flags::ADDITIVE, hit_points: 1, color: 0xff80_8080, sprite: Obj::Shockwave, ..Default::default() };
    d.smoke_effect.sprite = pats.get(ADD, 1);
    // Control35::Initialize (0x412820) and Control40::Initialize (0x413a40).
    let bullet = ActorDesc { color: 0xffff_ffff, flags: super::flags::FACE_START, team: 9, hit_points: 1, speed: 6.0, layer: 13, ..Default::default() };
    d.desc35 = ActorDesc { sprite: pats.get(SYS, 64), ..bullet };
    d.desc40 = ActorDesc { sprite: pats.get(SYS, 64), ..bullet };
    // Control41::Initialize (0x413e30).
    d.desc41 = ActorDesc { sprite: pats.get(SYS, 65), flags: 0, speed: 4.0, ..bullet };
    // Control42::Initialize (0x414040).
    d.beam_desc = ActorDesc { color: 0xffff_ffff, flags: super::flags::FACE_START | super::flags::ADDITIVE, team: 0x21, hit_points: 1, layer: 12, ..Default::default() };
    d.glow_desc = ActorDesc { color: 0xffff_ffff, sprite: pats.get(ADD, 16), flags: super::flags::ADDITIVE, hit_points: 1, layer: 12, ..Default::default() };
    // Control46::Initialize (0x414ba0).
    d.desc46 = ActorDesc { sprite: pats.get(ADD, 25), flags: super::flags::ADDITIVE, speed: 4.0, layer: 12, ..bullet };
    // Control49::Initialize (0x414e20).
    d.desc49 = ActorDesc { sprite: pats.get(SYS, 66), ..bullet };
    // Control32::Initialize (0x411500).
    d.boss_bounds = FloatRect { left: 64.0, top: 64.0, right: 576.0, bottom: 416.0 };
    cx.ctl = d;
}

/// ActorControl::OnCreate.
pub fn on_create(cx: &mut Cx, id: i16, a: Aid, param: Param) -> bool {
    match id {
        1 => player::on_create(cx, a),
        8 => fx::effect_create(cx, a, param),
        16 => fx::c16_create(cx, a),
        17 => fx::c17_create(cx, a),
        18 => fx::c18_create(cx, a),
        19 => fx::c19_create(cx, a),
        20 => fx::c20_create(cx, a),
        21 => cores::smoke_create(cx, a),
        22 => fx::glow_create(cx, a, param),
        23 => fx::flare_create(cx, a),
        32 => boss::body_create(cx, a, param),
        33 => boss::c33_create(cx, a, param),
        34 => boss::c34_create(cx, a, param),
        35 => boss::turret_create(cx, a, param),
        40..=42 | 44 | 46 | 47 | 49 => cores::core_create(cx, id, a, param),
        43 => cores::laser_create(cx, a, param),
        45 => cores::missile_create(cx, a, param),
        48 => cores::slow_missile_create(cx, a, param),
        _ => true,
    }
}

/// ActorControl::OnDestroy.
pub fn on_destroy(cx: &mut Cx, id: i16, a: Aid) {
    match id {
        1 => cx.game.on_player_destroyed(),
        16 => super::super::game_state::spawn_player(cx),
        20 => cx.game.boss = None,
        32 => boss::body_destroy(cx, a),
        33 | 34 => parts::part_destroy(cx, a),
        35 => cx.enm.turret_count -= 1,
        40..=42 | 44 | 46 | 47 | 49 => parts::core_destroy(cx, id, a),
        43 => cores::laser_destroy(cx, a),
        _ => {}
    }
}

/// ActorControl::OnMove: false when the actor is done.
pub fn on_move(cx: &mut Cx, id: i16, a: Aid) -> bool {
    match id {
        1 => player::on_move(cx, a),
        8 => fx::effect_move(cx, a),
        16 => cx.actors.get(a).age <= 60,
        17 => cx.actors.get(a).age <= 18,
        18 => fx::c18_move(cx, a),
        19 => fx::c19_move(cx, a),
        20 => fx::c20_move(cx, a),
        21 => cores::smoke_move(cx, a),
        22 => fx::glow_move(cx, a),
        23 => fx::flare_move(cx, a),
        32 => boss::body_move(cx, a),
        33 | 34 => parts::linked_move(cx, a),
        35 => boss::turret_move(cx, a),
        40..=42 | 44 | 46 | 47 | 49 => cores::core_move(cx, id, a),
        43 => cores::laser_move(cx, a),
        45 => cores::missile_move(cx, a),
        48 => cx.actors.get(a).speed > 0.0,
        _ => true,
    }
}

/// ActorControl::OnEvent.
pub fn on_event(cx: &mut Cx, id: i16, a: Aid, x: i32, y: i32) {
    match id {
        1 => player::on_event(cx, a, x, y),
        32 => boss::body_event(cx, a, x, y),
        33 | 34 => parts::linked_event(cx, a, x, y),
        40..=42 | 44 | 46 | 47 | 49 => parts::core_event(cx, id, a, x, y),
        43 => cores::laser_event(cx, a, x),
        _ => {}
    }
}

/// ActorControl::OnChildDestroyed.
pub fn on_child_destroyed(cx: &mut Cx, id: i16, parent: Aid, child: Aid) {
    match id {
        32 => boss::body_child_destroyed(cx, parent, child),
        33 | 34 => parts::linked_child_destroyed(cx, parent, child),
        _ => {}
    }
}

/// Sends the event (`x`, `y`) to `a` if it has a control (the inline
/// SendEvent of boss_controls.cpp).
pub fn send_event(cx: &mut Cx, a: Aid, x: i32, y: i32) {
    if a != 0 && a != super::NIL && cx.actors.get(a).control {
        let id = cx.actors.get(a).id;
        on_event(cx, id, a, x, y);
    }
}
