//! The actor controls (actor_control.h): one stateless behaviour per id,
//! registered by cGlobal::RegisterActorControls (0x40ff00). Here they are
//! a dispatch on the id; their static data lives in [`ControlData`].

use super::{ActorDesc, Aid, EffectDesc, FloatRect, Obj, Param, boss, cores, fx, parts, player};
use crate::game::Cx;

/// ControlIds: the ids of the controls (ActorSystem::Create, Actor::debris,
/// CORE_CONTROLS and DEBRIS_CONTROLS).
pub mod ids {
    /// 1: The player (player.rs).
    pub const PLAYER: i16 = 1;
    /// 8: An effect sprite run by an EffectDesc.
    pub const EFFECT: i16 = 8;
    /// 16: The player's explosion; respawns the player when it ends.
    pub const PLAYER_EXPLOSION: i16 = 16;
    /// 17: DEBRIS_CONTROLS[1]: an explosion with smoke (bomb_1).
    pub const SMALL_EXPLOSION: i16 = 17;
    /// 18: DEBRIS_CONTROLS[2]: creates a small explosion (bomb_2).
    pub const MEDIUM_EXPLOSION: i16 = 18;
    /// 19: DEBRIS_CONTROLS[3]: sparks, three shockwaves and a medium explosion (bomb_3).
    pub const LARGE_EXPLOSION: i16 = 19;
    /// 20: The debris of the boss body; clears the boss when done.
    pub const BOSS_EXPLOSION: i16 = 20;
    /// 21: The smoke trail of a missile.
    pub const SMOKE: i16 = 21;
    /// 22: The glow of an armored part.
    pub const GLOW: i16 = 22;
    /// 23: The flare of a part at a boosted dash.
    pub const FLARE: i16 = 23;
    /// 32: The body of the boss; runs the move, turn and fire programs.
    pub const BOSS_BODY: i16 = 32;
    /// 33: The four arms of the body (one per side and mirror).
    pub const BOSS_ARM: i16 = 33;
    /// 34: A linked (non-core) part of the boss that carries child parts.
    pub const JOINT_PART: i16 = 34;
    /// 35: The turret a linked part gets when its children are gone.
    pub const TURRET: i16 = 35;
    /// 40: Core of weapon 0, fires bullets.
    pub const BULLET_CORE: i16 = 40;
    /// 41: Core of weapon 1, fires a fan of shells.
    pub const SHELL_CORE: i16 = 41;
    /// 42: Core of weapon 2, fires a laser.
    pub const LASER_CORE: i16 = 42;
    /// 43: The laser beam of a laser core.
    pub const LASER: i16 = 43;
    /// 44: Core of weapon 3, launches homing missiles.
    pub const HOMING_CORE: i16 = 44;
    /// 45: A homing missile.
    pub const HOMING_MISSILE: i16 = 45;
    /// 46: Core of weapon 4, sprays sparks.
    pub const SPARK_CORE: i16 = 46;
    /// 47: Core of weapon 5, launches slow missiles.
    pub const MISSILE_CORE: i16 = 47;
    /// 48: A slow missile.
    pub const MISSILE: i16 = 48;
    /// 49: Core of weapon 6, a sweeping stream of bullets.
    pub const STREAM_CORE: i16 = 49;
}

/// The ids RegisterActorControls registers.
const REGISTERED: [i16; 24] = [
    ids::PLAYER,
    ids::EFFECT,
    ids::PLAYER_EXPLOSION,
    ids::SMALL_EXPLOSION,
    ids::MEDIUM_EXPLOSION,
    ids::LARGE_EXPLOSION,
    ids::BOSS_EXPLOSION,
    ids::SMOKE,
    ids::GLOW,
    ids::FLARE,
    ids::TURRET,
    ids::LASER,
    ids::BOSS_BODY,
    ids::BOSS_ARM,
    ids::JOINT_PART,
    ids::BULLET_CORE,
    ids::SHELL_CORE,
    ids::LASER_CORE,
    ids::HOMING_CORE,
    ids::MISSILE_CORE,
    ids::SPARK_CORE,
    ids::STREAM_CORE,
    ids::HOMING_MISSILE,
    ids::MISSILE,
];

/// The controls of the cores by weapon (0x45d91c).
pub const CORE_CONTROLS: [i16; 7] = [ids::BULLET_CORE, ids::SHELL_CORE, ids::LASER_CORE, ids::HOMING_CORE, ids::SPARK_CORE, ids::MISSILE_CORE, ids::STREAM_CORE];
/// The debris controls by size (0x45d938).
pub const DEBRIS_CONTROLS: [i16; 4] = [0, ids::SMALL_EXPLOSION, ids::MEDIUM_EXPLOSION, ids::LARGE_EXPLOSION];

pub fn registered(id: i32) -> bool {
    REGISTERED.iter().any(|&r| i32::from(r) == id)
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
    /// BossBodyControl::bounds_ (0x469054): where the boss body may move.
    pub boss_bounds: FloatRect,
    /// BossBodyControl::boss_ (0x469270): the body while it is alive.
    pub boss: Option<Aid>,
    /// BossBodyControl::weapons_ (0x469064).
    pub weapons: [i32; 7],
    /// CorePartControl::max_volley_time_ (0x469274).
    pub max_volley_time: i32,
    /// g_laser.
    pub laser: LaserSprite,
    /// PlayerControl::weapon_ (0x4690ec), kept from life to life.
    pub weapon: i32,
    /// PlayerControl::shot_ (0x4690b8).
    pub shot: ActorDesc,
    /// g_muzzle_effect (0x45d4ac).
    pub muzzle: EffectDesc,
    /// EffectControl::default_ (0x45d458).
    pub default_effect: EffectDesc,
    /// The bullets of TurretControl, BulletCoreControl, ShellCoreControl,
    /// SparkCoreControl and StreamCoreControl (their desc_).
    pub turret_desc: ActorDesc,
    pub bullet_core_desc: ActorDesc,
    pub shell_core_desc: ActorDesc,
    pub spark_core_desc: ActorDesc,
    pub stream_core_desc: ActorDesc,
    /// LaserControl::glow_desc_, beam_desc_.
    pub glow_desc: ActorDesc,
    pub beam_desc: ActorDesc,
    /// LaserControl::sound_users_ (0x469200).
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
    // PlayerControl::Initialize (0x412fe0).
    d.shot = ActorDesc { color: 0xffff_ffff, sprite: pats.get(SYS, 2), team: 8, debris: ids::EFFECT, flags: 0x11, hit_points: 1, speed: 16.0, layer: 10, ..Default::default() };
    d.muzzle.sprite = pats.get(ADD, 1);
    // EffectControl::Initialize (0x412c80).
    d.default_effect.sprite = pats.get(ADD, 1);
    // PlayerExplosionControl::Initialize (0x410540).
    d.fragment_desc = ActorDesc { color: 0xffff_ffff, age: 36, flags: super::flags::STOP_DIES, hit_points: 1, ..Default::default() };
    d.spark_desc = ActorDesc { color: 0xffff_ffff, flags: super::flags::FACE_START, hit_points: 1, sprite: pats.get(SYS, 8), layer: 0, speed: 8.0, acceleration: 2.0, ..Default::default() };
    d.shockwave_desc = ActorDesc { flags: super::flags::ADDITIVE, hit_points: 1, color: 0xff80_8080, sprite: Obj::Shockwave, ..Default::default() };
    d.smoke_effect.sprite = pats.get(ADD, 1);
    // TurretControl::Initialize (0x412820) and BulletCoreControl::Initialize (0x413a40).
    let bullet = ActorDesc { color: 0xffff_ffff, flags: super::flags::FACE_START, team: 9, hit_points: 1, speed: 6.0, layer: 13, ..Default::default() };
    d.turret_desc = ActorDesc { sprite: pats.get(SYS, 64), ..bullet };
    d.bullet_core_desc = ActorDesc { sprite: pats.get(SYS, 64), ..bullet };
    // ShellCoreControl::Initialize (0x413e30).
    d.shell_core_desc = ActorDesc { sprite: pats.get(SYS, 65), flags: 0, speed: 4.0, ..bullet };
    // LaserCoreControl::Initialize (0x414040).
    d.beam_desc = ActorDesc { color: 0xffff_ffff, flags: super::flags::FACE_START | super::flags::ADDITIVE, team: 0x21, hit_points: 1, layer: 12, ..Default::default() };
    d.glow_desc = ActorDesc { color: 0xffff_ffff, sprite: pats.get(ADD, 16), flags: super::flags::ADDITIVE, hit_points: 1, layer: 12, ..Default::default() };
    // SparkCoreControl::Initialize (0x414ba0).
    d.spark_core_desc = ActorDesc { sprite: pats.get(ADD, 25), flags: super::flags::ADDITIVE, speed: 4.0, layer: 12, ..bullet };
    // StreamCoreControl::Initialize (0x414e20).
    d.stream_core_desc = ActorDesc { sprite: pats.get(SYS, 66), ..bullet };
    // BossBodyControl::Initialize (0x411500).
    d.boss_bounds = FloatRect { left: 64.0, top: 64.0, right: 576.0, bottom: 416.0 };
    cx.ctl = d;
}

/// ActorControl::OnCreate.
pub fn on_create(cx: &mut Cx, id: i16, a: Aid, param: Param) -> bool {
    match id {
        ids::PLAYER => player::on_create(cx, a),
        ids::EFFECT => fx::effect_create(cx, a, param),
        ids::PLAYER_EXPLOSION => fx::player_explosion_create(cx, a),
        ids::SMALL_EXPLOSION => fx::small_explosion_create(cx, a),
        ids::MEDIUM_EXPLOSION => fx::medium_explosion_create(cx, a),
        ids::LARGE_EXPLOSION => fx::large_explosion_create(cx, a),
        ids::BOSS_EXPLOSION => fx::boss_explosion_create(cx, a),
        ids::SMOKE => cores::smoke_create(cx, a),
        ids::GLOW => fx::glow_create(cx, a, param),
        ids::FLARE => fx::flare_create(cx, a),
        ids::BOSS_BODY => boss::body_create(cx, a, param),
        ids::BOSS_ARM => boss::arm_create(cx, a, param),
        ids::JOINT_PART => boss::joint_part_create(cx, a, param),
        ids::TURRET => boss::turret_create(cx, a, param),
        ids::BULLET_CORE..=ids::LASER_CORE | ids::HOMING_CORE | ids::SPARK_CORE | ids::MISSILE_CORE | ids::STREAM_CORE => cores::core_create(cx, id, a, param),
        ids::LASER => cores::laser_create(cx, a, param),
        ids::HOMING_MISSILE => cores::homing_missile_create(cx, a, param),
        ids::MISSILE => cores::missile_create(cx, a, param),
        _ => true,
    }
}

/// ActorControl::OnDestroy.
pub fn on_destroy(cx: &mut Cx, id: i16, a: Aid) {
    match id {
        ids::PLAYER => cx.game.on_player_destroyed(),
        ids::PLAYER_EXPLOSION => super::super::game_state::spawn_player(cx),
        ids::BOSS_EXPLOSION => cx.game.boss = None,
        ids::BOSS_BODY => boss::body_destroy(cx, a),
        ids::BOSS_ARM | ids::JOINT_PART => parts::part_destroy(cx, a),
        ids::TURRET => cx.enm.turret_count -= 1,
        ids::BULLET_CORE..=ids::LASER_CORE | ids::HOMING_CORE | ids::SPARK_CORE | ids::MISSILE_CORE | ids::STREAM_CORE => parts::core_destroy(cx, id, a),
        ids::LASER => cores::laser_destroy(cx, a),
        _ => {}
    }
}

/// ActorControl::OnMove: false when the actor is done.
pub fn on_move(cx: &mut Cx, id: i16, a: Aid) -> bool {
    match id {
        ids::PLAYER => player::on_move(cx, a),
        ids::EFFECT => fx::effect_move(cx, a),
        ids::PLAYER_EXPLOSION => cx.actors.get(a).age <= 60,
        ids::SMALL_EXPLOSION => cx.actors.get(a).age <= 18,
        ids::MEDIUM_EXPLOSION => fx::medium_explosion_move(cx, a),
        ids::LARGE_EXPLOSION => fx::large_explosion_move(cx, a),
        ids::BOSS_EXPLOSION => fx::boss_explosion_move(cx, a),
        ids::SMOKE => cores::smoke_move(cx, a),
        ids::GLOW => fx::glow_move(cx, a),
        ids::FLARE => fx::flare_move(cx, a),
        ids::BOSS_BODY => boss::body_move(cx, a),
        ids::BOSS_ARM | ids::JOINT_PART => parts::linked_move(cx, a),
        ids::TURRET => boss::turret_move(cx, a),
        ids::BULLET_CORE..=ids::LASER_CORE | ids::HOMING_CORE | ids::SPARK_CORE | ids::MISSILE_CORE | ids::STREAM_CORE => cores::core_move(cx, id, a),
        ids::LASER => cores::laser_move(cx, a),
        ids::HOMING_MISSILE => cores::homing_missile_move(cx, a),
        ids::MISSILE => cx.actors.get(a).speed > 0.0,
        _ => true,
    }
}

/// ActorControl::OnEvent.
pub fn on_event(cx: &mut Cx, id: i16, a: Aid, event: i32, targets: i32) {
    match id {
        ids::PLAYER => player::on_event(cx, a, event, targets),
        ids::BOSS_BODY => boss::body_event(cx, a, event, targets),
        ids::BOSS_ARM | ids::JOINT_PART => parts::linked_event(cx, a, event, targets),
        ids::BULLET_CORE..=ids::LASER_CORE | ids::HOMING_CORE | ids::SPARK_CORE | ids::MISSILE_CORE | ids::STREAM_CORE => parts::core_event(cx, id, a, event, targets),
        ids::LASER => cores::laser_event(cx, a, event),
        _ => {}
    }
}

/// ActorControl::OnChildDestroyed.
pub fn on_child_destroyed(cx: &mut Cx, id: i16, parent: Aid, child: Aid) {
    match id {
        ids::BOSS_BODY => boss::body_child_destroyed(cx, parent, child),
        ids::BOSS_ARM | ids::JOINT_PART => parts::linked_child_destroyed(cx, parent, child),
        _ => {}
    }
}

/// Sends `event` to `a` (`targets`: which parts take it) if it has a
/// control (the inline SendEvent of boss_controls.cpp).
pub fn send_event(cx: &mut Cx, a: Aid, event: i32, targets: i32) {
    if a != 0 && a != super::NIL && cx.actors.get(a).control {
        let id = cx.actors.get(a).id;
        on_event(cx, id, a, event, targets);
    }
}
