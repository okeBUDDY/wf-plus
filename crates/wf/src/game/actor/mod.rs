//! The actor system (actor_system.cpp, actor.h): a fixed pool of actors
//! in a tree, each on the draw list of its layer, run by the control
//! registered for its id (controls.rs).
//!
//! Pointers of the original become indices: an actor is an [`Aid`] into
//! the pool (0 is the root), a plan slot an index into `EnmParam::slots`.
//! `Actor::work` keeps the original's 64 bytes of control state; typed
//! views of it ([`ActorSystem::w`]) replace the reinterpret_casts.

pub mod controls;
mod cores;
mod boss;
mod fx;
pub mod parts;
pub mod player;

use super::Cx;
use super::math::{self, cos, sin};
use crate::gfx::{Argb, Blend, Primitive, SpriteId, Transform};
use bytemuck::Pod;

/// An actor: an index into the pool (0 is the root).
pub type Aid = u16;
/// No actor (a NULL link).
pub const NIL: Aid = u16::MAX;

/// Actor::Flags.
pub mod flags {
    pub const FACE_START: u32 = 0x1;
    pub const STOP_DIES: u32 = 0x2;
    pub const MIRROR_X: u32 = 0x4;
    pub const MIRROR_Y: u32 = 0x8;
    pub const ADDITIVE: u32 = 0x10;
    pub const CREATING: u32 = 0x20000;
    pub const MOVED: u32 = 0x40000;
    pub const NEW: u32 = 0x70000;
    pub const CHILD: u32 = 0x80000;
    pub const DEAD: u32 = 0x100000;
    pub const DEBRIS: u32 = 0x200000;
    pub const MOVED_ONCE: u32 = 0x400000;
}

/// Actor::Team.
pub mod team {
    pub const TEAM_MASK: u16 = 0x7;
    pub const HITS: u16 = 0x8;
    pub const HIT_BY_OTHERS: u16 = 0x10;
    pub const HITTABLE: u16 = 0x20;
    pub const DAMAGE_MASK: u16 = 0xc0;
    pub const DAMAGE_SELF: u16 = 0x0;
    pub const DAMAGE_ARMOR: u16 = 0x40;
    pub const DAMAGE_PARENT: u16 = 0x80;
    pub const DAMAGE_GUARD: u16 = 0xc0;
}

/// The pattern sheets (g_pats): sys0, enm0, add0, etc0..etc2. The plan's
/// pattern file `i` is sheet `ENM + i` (PlanPatFile()).
pub mod sheet {
    pub const SYS: u8 = 0;
    pub const ENM: u8 = 1;
    pub const ADD: u8 = 2;
    pub const ETC0: u8 = 3;
}

/// A pattern of a sheet (HLSPat).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatId {
    pub sheet: u8,
    pub id: u16,
}

/// What an actor draws (HLSObject and its subclasses).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Obj {
    /// g_null_sprite.
    #[default]
    Null,
    Pat(PatId),
    /// An animation of the pool (HLSAnm).
    Anm(u16),
    /// The triangle of a shockwave ring (g_shockwave).
    Shockwave,
    /// The player's laser (g_laser).
    Laser,
    /// A text (HLSTxt) of `Cx::obj_texts`, printed at the actor.
    Text(u8),
}

/// The cooked art and the hit box of a pattern.
#[derive(Clone, Copy, Debug, Default)]
pub struct PatInfo {
    pub sprite: Option<SpriteId>,
    /// Cuts `sprite` to a rect of its space (a pattern that is part of
    /// a joined vector sprite).
    pub clip: Option<[f32; 4]>,
    /// Drawn over `sprite` (original pixels kept as a bitmap).
    pub overlay: Option<SpriteId>,
    /// Half the hit box (HLSPat::hit_width_, hit_height_).
    pub hit: [u16; 2],
    /// Nothing to draw (a 0 x 0 pattern).
    pub empty: bool,
}

/// The patterns of every sheet (g_pats), indexed by id.
#[derive(Default)]
pub struct Pats {
    pub sheets: Vec<Vec<Option<PatInfo>>>,
}

/// Patterns whose art was joined or split when it was vectorized:
/// (sheet, pattern, sprite, clip in the sprite's space, bitmap overlay).
/// etc2_logo joins etc2 patterns 1 (sheet x 0..256) and 2 (x 256..320)
/// around their common pivot (160, 56), less the character, which is kept
/// as the original pixels.
const JOINED: &[(&str, u32, &str, [f32; 4], Option<&str>)] = &[
    ("etc2", 1, "etc2_logo", [-160.0, -56.0, 96.0, 56.0], Some("etc2_001_character")),
    ("etc2", 2, "etc2_logo", [96.0, -56.0, 160.0, 56.0], None),
];

impl Pats {
    /// Joins the .pat files of data.bin with the cooked art.
    pub fn load(assets: &super::Assets, art: &crate::art::Art) -> Self {
        let mut sheets = Vec::new();
        for name in super::assets::SHEETS {
            let mut v = Vec::new();
            if let Some(pat) = assets.pat(name) {
                for (id, p) in pat.patterns.iter().enumerate() {
                    v.push(p.map(|p| {
                        let mut info = PatInfo {
                            sprite: art.pattern(name, id as u32),
                            clip: None,
                            overlay: None,
                            hit: p.hit_half,
                            empty: p.rect[2] == 0 && p.rect[3] == 0,
                        };
                        if let Some(j) = JOINED.iter().find(|j| j.0 == name && j.1 == id as u32) {
                            info.sprite = art.find(j.2);
                            info.clip = Some(j.3);
                            info.overlay = j.4.and_then(|o| art.find(o));
                        }
                        info
                    }));
                }
            }
            sheets.push(v);
        }
        Pats { sheets }
    }

    /// HLSPatFile::Get: the object of pattern `id` (the null sprite when
    /// the sheet has none).
    pub fn get(&self, sheet: u8, id: i32) -> Obj {
        match self.info(PatId { sheet, id: id as u16 }) {
            Some(_) if id >= 0 => Obj::Pat(PatId { sheet, id: id as u16 }),
            _ => Obj::Null,
        }
    }

    pub fn info(&self, p: PatId) -> Option<&PatInfo> {
        self.sheets.get(p.sheet as usize)?.get(p.id as usize)?.as_ref()
    }
}

/// An animation script (AnmData): (pattern, frames) pairs of `pat_file`;
/// -2 jumps (target, count), -1 ends.
#[derive(Debug)]
pub struct AnmData {
    /// The pattern file (sheet) whose patterns the script numbers
    /// (AnmData::pat_file).
    pub pat_file: u8,
    pub script: &'static [i16],
}

/// HLSAnm: plays an AnmData.
#[derive(Clone, Copy, Debug, Default)]
pub struct Anm {
    data: Option<&'static AnmData>,
    /// The current entry (cursor_), None while free.
    cursor: Option<usize>,
    object: Option<PatId>,
    texture: u8,
    wait: i16,
    loop_: i16,
}

impl Anm {
    fn script(&self, at: usize) -> i16 {
        self.data.map_or(-1, |d| d.script.get(at).copied().unwrap_or(-1))
    }

    /// HLSAnm::Step (0x407420).
    fn step(&mut self) {
        let Some(mut c) = self.cursor else { return };
        if self.wait <= 0 {
            return;
        }
        self.wait -= 1;
        if self.wait > 0 {
            return;
        }
        c += 2;
        let mut command;
        loop {
            command = self.script(c);
            if command != -2 {
                break;
            }
            match self.loop_ {
                -1 => {}
                0 => self.loop_ = self.script(c + 2),
                1 => {
                    self.loop_ = 0;
                    c += 3;
                    continue;
                }
                _ => self.loop_ -= 1,
            }
            let target = self.script(c + 1);
            if target >= 0 {
                c = target as usize;
            } else {
                c = (c as isize - target as isize) as usize;
            }
        }
        self.cursor = Some(c);
        if command == -1 {
            return;
        }
        let pat_file = self.data.map_or(0, |d| d.pat_file);
        self.object = Some(PatId { sheet: pat_file, id: command as u16 });
        self.wait = self.script(c + 1);
    }
}

/// HLSHitImage: the actors' ids at a quarter of the screen resolution.
pub struct HitImage {
    bits: Vec<u16>,
    width: i32,
    height: i32,
    shift: i32,
}

impl HitImage {
    pub fn size(&self) -> (i32, i32) {
        (self.width << self.shift, self.height << self.shift)
    }

    pub const MAX_HEIGHT: i32 = 120;

    /// HLSHitImage::Create (0x407650).
    pub fn new(mut width: i32, mut height: i32) -> Self {
        let mut shift = 0;
        while height > Self::MAX_HEIGHT {
            width >>= 1;
            height >>= 1;
            shift += 1;
        }
        HitImage { bits: vec![0; (width * height) as usize], width, height, shift }
    }

    pub fn clear(&mut self) {
        self.bits.fill(0);
    }

    /// HLSHitImage::Fill (0x407720).
    pub fn fill(&mut self, x: i32, y: i32, width: i32, height: i32, id: u16) {
        let mut x = (x - width) >> self.shift;
        let mut y = (y - height) >> self.shift;
        let mut width = (width >> self.shift) * 2;
        let mut height = (height >> self.shift) * 2;
        if x + width <= 0 || x >= self.width || y + height <= 0 || y >= self.height {
            return;
        }
        if x < 0 {
            width += x;
            x = 0;
        }
        if y < 0 {
            height += y;
            y = 0;
        }
        if x + width >= self.width {
            width = self.width - x;
        }
        if y + height >= self.height {
            height = self.height - y;
        }
        if width == 0 || height == 0 {
            return;
        }
        for row in y..y + height {
            let at = (row * self.width + x) as usize;
            self.bits[at..at + width as usize].fill(id);
        }
    }

    fn at(&self, sx: f32, sy: f32) -> u16 {
        let x = math::ftol(sx) >> self.shift;
        let y = math::ftol(sy) >> self.shift;
        if x >= 0 && x < self.width && y >= 0 && y < self.height { self.bits[(y * self.width + x) as usize] } else { 0 }
    }
}

/// A rectangle in screen coordinates (FloatRect).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FloatRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// What `ActorSystem::spawn` makes an actor from (ActorDesc).
#[derive(Clone, Copy, Debug, Default)]
pub struct ActorDesc {
    pub flags: u32,
    pub sprite: Obj,
    pub parent: Option<Aid>,
    pub color: u32,
    pub speed: f32,
    pub acceleration: f32,
    /// The EnmSlot of the firing part (an index); Actor::Restore stores it
    /// in work[0], where on_actor_destroyed reads the shooter's slot.
    pub slot: u32,
    pub x: f32,
    pub y: f32,
    pub layer: u16,
    pub team: u16,
    pub hit_points: i16,
    pub debris: i16,
    pub angle: u16,
    pub start_angle: u16,
    pub age: u16,
}

/// How a control moves and fades an effect sprite (EffectDesc, EffectControl).
#[derive(Clone, Copy, Debug, Default)]
pub struct EffectDesc {
    pub sprite: Obj,
    pub flags: u16,
    pub life: u16,
    pub scale_min: f32,
    pub scale_max: f32,
    pub grow: f32,
    pub speed_min: f32,
    pub speed_max: f32,
    pub acceleration: f32,
    pub angle_min: i16,
    pub angle_max: i16,
    pub color: [i16; 4],
    pub fade: [i16; 4],
}

/// The `param` of ActorControl::OnCreate.
#[derive(Clone, Copy, Debug, Default)]
pub enum Param {
    #[default]
    None,
    Int(i32),
    Effect(EffectDesc),
    /// An EnmSlot (index into EnmParam::slots).
    Slot(u16),
    /// The work of another actor (the launcher of a missile, ...).
    Actor(Aid),
}

/// One actor (0x120 bytes in the original).
#[derive(Clone, Debug)]
pub struct Actor {
    pub flags: u32,
    pub hit_points: i32,
    pub low_hit_points: i32,
    pub team: u16,
    pub age: u16,
    pub hit_age: u16,
    pub debris: i16,
    pub angle: i16,
    pub start_angle: u16,
    pub speed: f32,
    pub acceleration: f32,
    /// Kept inside the boss's bounds (BossBodyControl::bounds_), else it dies
    /// outside the margin of the screen.
    pub bounded: bool,
    pub sprite: Obj,
    pub color: u32,
    pub work: [u32; 16],
    /// Has a registered control (control != NULL).
    pub control: bool,
    pub hit_id: u16,
    pub id: i16,
    pub layer: u16,
    pub matrix_angle: i16,
    pub scale: f32,
    pub parent: Aid,
    pub next_sibling: Aid,
    pub prev_sibling: Aid,
    pub first_child: Aid,
    pub last_child: Aid,
    pub draw_prev: Aid,
    pub draw_next: Aid,
    /// `transform == &relative`.
    pub relative_tf: bool,
    pub screen: Transform,
    pub relative: Transform,
}

impl Default for Actor {
    /// Actor::Actor (0x407ce0) and Reset (0x407c80).
    fn default() -> Self {
        Actor {
            flags: 0,
            hit_points: 1,
            low_hit_points: 0,
            team: 0,
            age: 0,
            hit_age: 0,
            debris: 0,
            angle: 0,
            start_angle: 0,
            speed: 0.0,
            acceleration: 0.0,
            bounded: false,
            sprite: Obj::Null,
            color: 0xffff_ffff,
            work: [0; 16],
            control: false,
            hit_id: 0,
            id: 0,
            layer: 0,
            matrix_angle: 0,
            scale: 1.0,
            parent: NIL,
            next_sibling: NIL,
            prev_sibling: NIL,
            first_child: NIL,
            last_child: NIL,
            draw_prev: NIL,
            draw_next: NIL,
            relative_tf: false,
            screen: Transform::IDENTITY,
            relative: Transform::IDENTITY,
        }
    }
}

impl Actor {
    /// `*transform`.
    pub fn tf(&self) -> &Transform {
        if self.relative_tf { &self.relative } else { &self.screen }
    }
    pub fn tf_mut(&mut self) -> &mut Transform {
        if self.relative_tf { &mut self.relative } else { &mut self.screen }
    }
    pub fn x(&self) -> f32 {
        self.tf().m[2][0]
    }
    pub fn y(&self) -> f32 {
        self.tf().m[2][1]
    }
    pub fn sx(&self) -> f32 {
        self.screen.m[2][0]
    }
    pub fn sy(&self) -> f32 {
        self.screen.m[2][1]
    }
    pub fn set_position(&mut self, x: f32, y: f32) {
        let t = self.tf_mut();
        t.m[2][0] = x;
        t.m[2][1] = y;
        self.flags |= flags::MOVED;
    }
    /// Actor::SetScale: the matrix is rebuilt at the next move.
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
        self.matrix_angle = self.matrix_angle.wrapping_add(0x1000);
    }
    /// Actor::LerpColor (0x407900): `num`/`den` of the way from `from` to
    /// `to`, per byte.
    pub fn lerp_color(&mut self, from: u32, to: u32, num: i32, den: i32) {
        let mut c = 0u32;
        for shift in [0, 8, 16, 24] {
            let a = ((from >> shift) & 0xff) as i32;
            let b = ((to >> shift) & 0xff) as i32;
            let v = a + (b - a) * num / den;
            c |= ((v as u32) & 0xff) << shift;
        }
        self.color = c;
    }
}

/// Actor::ClampAngle (0x418ef0).
pub fn clamp_angle(value: u16, low: i16, high: i16) -> u16 {
    let mut result = low as u16;
    let range = high.wrapping_sub(low);
    if range > 0 {
        if range >= 0x400 {
            return value;
        }
        let offset = (value.wrapping_sub(low as u16) & 0x3ff) as i16;
        if offset <= range {
            return value;
        }
        if (offset as i32) < ((range >> 1) as i32) + 0x200 {
            result = high as u16;
        }
    }
    result
}

/// ActorSystem: the pool, the draw lists, the hit image and the
/// animation pool (HLSAnmSys).
pub struct ActorSystem {
    pub a: Vec<Actor>,
    max_actors: usize,
    reserved: usize,
    pub count: i16,
    last_dead: i32,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    layers: [Aid; Self::LAYERS],
    pub hit: HitImage,
    pub anims: Vec<Anm>,
    /// ActorSystem::flags_: 1 while actors hit each other.
    pub flags: i32,
}

impl ActorSystem {
    pub const LAYERS: usize = 16;

    /// set_limits(0x80, 0x600, 8) and Initialize (0x408970) for a 640x480
    /// screen, SetMargin(100, 32), g_anm_sys with 0x200 animations.
    pub fn new() -> Self {
        let max_actors = 0x600;
        let mut a: Vec<Actor> = (0..=max_actors).map(|_| Actor::default()).collect();
        for (i, actor) in a.iter_mut().enumerate() {
            actor.hit_id = i as u16;
        }
        let root = &mut a[0];
        root.flags = flags::DEAD;
        root.low_hit_points = 0;
        let mut s = ActorSystem {
            a,
            max_actors,
            reserved: 8,
            count: 0,
            last_dead: 0,
            left: -100.0,
            top: -32.0,
            right: 740.0,
            bottom: 512.0,
            layers: [NIL; Self::LAYERS],
            hit: HitImage::new(640, 480),
            anims: vec![Anm::default(); 0x200],
            flags: 1,
        };
        s.flags = 1;
        s
    }

    /// A typed view of the work of actor `a`.
    pub fn w<T: Pod>(&mut self, a: Aid) -> &mut T {
        let bytes: &mut [u8] = bytemuck::cast_slice_mut(&mut self.a[a as usize].work);
        bytemuck::from_bytes_mut(&mut bytes[..std::mem::size_of::<T>()])
    }
    pub fn wr<T: Pod>(&self, a: Aid) -> &T {
        let bytes: &[u8] = bytemuck::cast_slice(&self.a[a as usize].work);
        bytemuck::from_bytes(&bytes[..std::mem::size_of::<T>()])
    }

    pub fn get(&self, a: Aid) -> &Actor {
        &self.a[a as usize]
    }
    pub fn get_mut(&mut self, a: Aid) -> &mut Actor {
        &mut self.a[a as usize]
    }

    /// The texture an object draws from (HLSObject::texture_), for Link.
    fn texture(&self, o: Obj) -> u8 {
        match o {
            Obj::Pat(p) => p.sheet + 1,
            Obj::Anm(i) => self.anims[i as usize].texture,
            // The font sheets come after the six pattern sheets.
            Obj::Text(_) => 0x20,
            _ => 0,
        }
    }

    /// HLSObject::Reset of an actor's sprite (frees an animation).
    fn reset_sprite(&mut self, o: Obj) {
        if let Obj::Anm(i) = o {
            self.anims[i as usize].cursor = None;
        }
    }

    /// Actor::SetSprite.
    pub fn set_sprite(&mut self, a: Aid, sprite: Obj) {
        let old = self.a[a as usize].sprite;
        self.reset_sprite(old);
        self.a[a as usize].sprite = sprite;
    }

    /// HLSAnmSys::Play (0x410850): an animation of `data`, or the null
    /// sprite when the pool is used up.
    pub fn play(&mut self, data: &'static AnmData) -> Obj {
        for (i, anim) in self.anims.iter_mut().enumerate() {
            if anim.cursor.is_none() {
                anim.data = Some(data);
                anim.cursor = Some(0);
                anim.object = Some(PatId { sheet: data.pat_file, id: data.script[0] as u16 });
                anim.texture = data.pat_file + 1;
                anim.wait = data.script[1];
                anim.loop_ = 0;
                return Obj::Anm(i as u16);
            }
        }
        Obj::Null
    }

    /// Actor::Reset (0x407c80).
    fn reset(&mut self, a: Aid) {
        let old = self.a[a as usize].sprite;
        self.reset_sprite(old);
        let x = &mut self.a[a as usize];
        x.team = 0;
        x.id = 0;
        x.hit_age = 0;
        x.age = 0;
        x.debris = 0;
        x.hit_points = 1;
        x.angle = 0;
        x.start_angle = 0;
        x.acceleration = 0.0;
        x.speed = 0.0;
        x.scale = 1.0;
        x.bounded = false;
        x.last_child = NIL;
        x.first_child = NIL;
        x.sprite = Obj::Null;
        x.color = 0xffff_ffff;
    }

    /// ActorSystem::Link (0x407b60).
    fn link(&mut self, a: Aid) {
        let layer = self.a[a as usize].layer as usize;
        let head = self.layers[layer];
        if head != NIL {
            let texture = self.texture(self.a[a as usize].sprite);
            let mut other = head;
            while self.a[other as usize].draw_next != NIL && self.texture(self.a[other as usize].sprite) != texture {
                other = self.a[other as usize].draw_next;
            }
            let next = self.a[other as usize].draw_next;
            self.a[a as usize].draw_next = next;
            self.a[a as usize].draw_prev = other;
            self.a[other as usize].draw_next = a;
            if next != NIL {
                self.a[next as usize].draw_prev = a;
            }
        } else {
            self.layers[layer] = a;
            self.a[a as usize].draw_next = NIL;
            self.a[a as usize].draw_prev = NIL;
        }
    }

    fn unlink(&mut self, a: Aid) {
        let (prev, next, layer) = {
            let x = &self.a[a as usize];
            (x.draw_prev, x.draw_next, x.layer as usize)
        };
        if prev != NIL {
            self.a[prev as usize].draw_next = next;
        } else {
            self.layers[layer] = next;
        }
        if next != NIL {
            self.a[next as usize].draw_prev = prev;
        }
    }

    /// Actor::SetLayer (0x4108c0).
    pub fn set_layer(&mut self, a: Aid, layer: u16) {
        if self.a[a as usize].flags & flags::CREATING != 0 {
            self.a[a as usize].layer = layer;
            return;
        }
        self.unlink(a);
        self.a[a as usize].layer = layer;
        self.link(a);
    }

    /// Actor::Flash (0x407b30).
    fn flash(&mut self, a: Aid) {
        if self.a[a as usize].hit_points > 0 {
            self.a[a as usize].hit_points = -self.a[a as usize].hit_points;
            let mut child = self.a[a as usize].first_child;
            while child != NIL {
                self.flash(child);
                child = self.a[child as usize].prev_sibling;
            }
        }
    }

    /// Actor::HitTest (0x407aa0).
    pub fn hit_test(&self, a: Aid) -> Option<Aid> {
        let x = &self.a[a as usize];
        let id = self.hit.at(x.sx(), x.sy()) as i16;
        if id != 0 {
            let other = &self.a[(id as u16) as usize];
            if other.flags != flags::DEAD && ((other.team ^ x.team) & team::TEAM_MASK) != 0 {
                return Some(id as u16);
            }
        }
        None
    }

    /// The culling rect and the hit image of `arena`.
    pub fn set_arena(&mut self, arena: &crate::game::arena::Arena) {
        [self.left, self.top, self.right, self.bottom] = arena.cull();
        let (w, h) = arena.hit_size();
        if self.hit.size() != (w, h) {
            self.hit = HitImage::new(w, h);
        }
    }

    /// Actor::ScreenAngle (0x4113d0).
    pub fn screen_angle(&self, a: Aid) -> u16 {
        let x = &self.a[a as usize];
        if !x.relative_tf {
            return x.angle as u16;
        }
        let mut result = x.angle as u16;
        let mut anc = x.parent;
        while anc != 0 && anc != NIL {
            let p = &self.a[anc as usize];
            if p.flags & flags::MIRROR_X != 0 {
                result = result.wrapping_neg();
            }
            if p.flags & flags::MIRROR_Y != 0 {
                result = 0x200u16.wrapping_sub(result);
            }
            result = result.wrapping_add(p.angle as u16);
            anc = p.parent;
        }
        result
    }

    /// Actor::ToLocalAngle (0x411690).
    pub fn to_local_angle(&self, a: Aid, mut screen_angle: u16) -> u16 {
        let x = &self.a[a as usize];
        if x.relative_tf {
            screen_angle = self.to_local_angle(x.parent, screen_angle);
        }
        screen_angle = screen_angle.wrapping_sub(x.angle as u16);
        if x.flags & flags::MIRROR_X != 0 {
            screen_angle = screen_angle.wrapping_neg();
        }
        if x.flags & flags::MIRROR_Y != 0 {
            screen_angle = 0x200u16.wrapping_sub(screen_angle);
        }
        screen_angle
    }

    /// Actor::AngleTo (0x411d90, matched): the angle to (`x`, `y`) in the
    /// actor's parent's frame.
    pub fn angle_to(&self, a: Aid, x: f32, y: f32) -> i16 {
        let act = &self.a[a as usize];
        if act.flags & flags::MOVED_ONCE == 0 {
            return act.angle;
        }
        let dx = x - act.sx();
        let dy = y - act.sy();
        if dx == 0.0 && dy == 0.0 {
            return act.angle;
        }
        let result = math::angle_of(dx, dy);
        (if act.relative_tf { self.to_local_angle(act.parent, result) } else { result }) as i16
    }

    /// Actor::TurnTo (0x414a00).
    pub fn turn_to(&mut self, a: Aid, target: Option<Aid>, speed: i16) {
        let goal = match target {
            Some(t) => {
                let (tx, ty) = (self.a[t as usize].sx(), self.a[t as usize].sy());
                self.angle_to(a, tx, ty)
            }
            None => self.a[a as usize].angle,
        };
        let x = &mut self.a[a as usize];
        if speed >= 0x200 {
            x.angle = goal;
            return;
        }
        let mut offset = goal.wrapping_sub(x.angle) & 0x3ff;
        if offset >= 0x200 {
            offset -= 0x400;
        }
        if offset.abs() < speed {
            x.angle = x.angle.wrapping_add(offset);
        } else if offset < 0 {
            x.angle = x.angle.wrapping_sub(speed);
        } else {
            x.angle = x.angle.wrapping_add(speed);
        }
    }

    /// Rebuilds the rotation of the transform (Init and Move).
    fn rotate(x: &mut Actor, scaled: bool) {
        let angle = x.angle as i32;
        let scale = x.scale;
        let t = x.tf_mut();
        if scaled {
            t.m[0][1] = sin(angle) * scale;
            t.m[1][0] = -t.m[0][1];
            t.m[1][1] = cos(angle) * scale;
        } else {
            t.m[0][1] = sin(angle);
            t.m[1][0] = -t.m[0][1];
            t.m[1][1] = cos(angle);
        }
        t.m[0][0] = t.m[1][1];
    }
}

impl Default for ActorSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Actor::Init (0x407d40).
fn init(cx: &mut Cx, a: Aid, control: bool, parent: Option<Aid>, x: f32, y: f32, layer: u16, angle: u16, param: Param) -> bool {
    {
        let act = cx.actors.get_mut(a);
        act.flags &= 0xffff;
        if parent.is_none() {
            act.relative_tf = false;
            act.screen.m[1][2] = 0.0;
            act.screen.m[0][2] = 0.0;
            act.screen.m[2][2] = 1.0;
        } else {
            act.relative_tf = true;
            act.flags |= flags::CHILD;
        }
        act.control = control;
        act.parent = parent.unwrap_or(0);
        let t = act.tf_mut();
        t.m[2][0] = x;
        t.m[2][1] = y;
        act.flags |= flags::NEW;
        act.layer = layer;
        act.start_angle = angle;
    }
    if control {
        let id = cx.actors.get(a).id;
        if !controls::on_create(cx, id, a, param) {
            cx.actors.get_mut(a).flags = 0;
            return false;
        }
    }
    let parent = {
        let act = cx.actors.get_mut(a);
        if act.flags & flags::FACE_START != 0 {
            act.angle = act.start_angle as i16;
        }
        act.matrix_angle = act.angle.wrapping_add(0x1000);
        act.low_hit_points = act.hit_points >> 3;
        ActorSystem::rotate(act, true);
        act.prev_sibling = NIL;
        act.parent
    };
    let s = &mut cx.actors;
    let last = s.a[parent as usize].last_child;
    if last != NIL {
        s.a[a as usize].next_sibling = last;
        s.a[parent as usize].last_child = a;
        s.a[last as usize].prev_sibling = a;
    } else {
        s.a[a as usize].next_sibling = NIL;
        s.a[parent as usize].last_child = a;
        s.a[parent as usize].first_child = a;
    }
    s.link(a);
    s.count += 1;
    s.a[a as usize].flags &= !flags::CREATING;
    true
}

/// ActorSystem::Create (0x4080f0): an actor run by control `id` (a
/// negative id may take the reserved slots too).
pub fn create(cx: &mut Cx, id: i32, parent: Option<Aid>, x: f32, y: f32, layer: u16, angle: u16, param: Param) -> Option<Aid> {
    let (id, start, count) = if id < 0 {
        (-id, 1, cx.actors.max_actors)
    } else {
        (id, cx.actors.reserved + 1, cx.actors.max_actors - cx.actors.reserved)
    };
    let slot = (start..start + count).find(|&i| cx.actors.a[i].flags == 0)?;
    let a = slot as Aid;
    cx.actors.a[slot].id = id as i16;
    let control = controls::registered(id);
    if init(cx, a, control, parent, x, y, layer, angle, param) {
        return Some(a);
    }
    cx.actors.a[slot].id = 0;
    None
}

/// ActorSystem::Spawn (0x408190) and Actor::Restore (0x407ef0): an actor
/// without a control.
pub fn spawn(cx: &mut Cx, desc: &ActorDesc) -> Option<Aid> {
    let start = cx.actors.reserved + 1;
    let count = cx.actors.max_actors - cx.actors.reserved;
    let slot = (start..start + count).find(|&i| cx.actors.a[i].flags == 0)?;
    let a = slot as Aid;
    cx.actors.set_sprite(a, desc.sprite);
    {
        let act = cx.actors.get_mut(a);
        act.flags = desc.flags;
        act.hit_points = desc.hit_points as i32;
        act.angle = desc.angle as i16;
    }
    if !init(cx, a, false, desc.parent, desc.x, desc.y, desc.layer, desc.start_angle, Param::None) {
        return None;
    }
    let act = cx.actors.get_mut(a);
    act.color = desc.color;
    act.team = desc.team;
    act.speed = desc.speed;
    act.acceleration = desc.acceleration;
    act.debris = desc.debris;
    act.age = desc.age;
    act.work[0] = desc.slot;
    Some(a)
}

/// Actor::Destroy (0x4082b0).
pub fn destroy(cx: &mut Cx, a: Aid, debris: bool, notify: bool) {
    let parent = cx.actors.get(a).parent;
    if notify && cx.actors.get(parent).control {
        let pid = cx.actors.get(parent).id;
        controls::on_child_destroyed(cx, pid, parent, a);
    }
    if cx.actors.get(a).control {
        let id = cx.actors.get(a).id;
        controls::on_destroy(cx, id, a);
    }
    cx.actors.get_mut(a).flags = flags::DEAD;
    while cx.actors.get(a).first_child != NIL {
        let child = cx.actors.get(a).first_child;
        destroy(cx, child, debris, false);
    }
    let s = &mut cx.actors;
    let (next, prev, parent) = {
        let x = &s.a[a as usize];
        (x.next_sibling, x.prev_sibling, x.parent)
    };
    if next != NIL {
        s.a[next as usize].prev_sibling = prev;
    } else {
        s.a[parent as usize].first_child = prev;
    }
    if prev != NIL {
        s.a[prev as usize].next_sibling = next;
    } else {
        s.a[parent as usize].last_child = next;
    }
    s.unlink(a);
    let x = s.a[a as usize].clone();
    if debris && x.debris != 0 {
        if x.debris > 0 {
            create(cx, x.debris as i32, None, x.sx(), x.sy(), x.layer, 0, Param::None);
        } else if cx.actors.get(x.parent).flags == flags::DEAD {
            if let Some(d) = create(cx, -(x.debris as i32), None, x.sx(), x.sy(), x.layer, 0, Param::None) {
                cx.actors.get_mut(d).flags |= flags::DEBRIS;
            }
        } else {
            create(cx, -(x.debris as i32), Some(x.parent), x.x(), x.y(), x.layer, 0, Param::None);
        }
    }
    cx.actors.reset(a);
    cx.actors.count -= 1;
    if x.hit_id as i32 > cx.actors.last_dead {
        cx.actors.last_dead = x.hit_id as i32;
    }
}

/// ActorSystem::DestroyAll (0x408480).
pub fn destroy_all(cx: &mut Cx) {
    cx.actors.flags |= 1;
    let mut actor = cx.actors.get(0).first_child;
    while actor != NIL {
        let next = cx.actors.get(actor).prev_sibling;
        destroy(cx, actor, false, false);
        actor = next;
    }
}

/// Actor::Damage (0x4084b0).
pub fn damage(cx: &mut Cx, a: Aid, amount: i32, source: Aid) -> i32 {
    let mut actor = a;
    loop {
        match cx.actors.get(actor).team & team::DAMAGE_MASK {
            team::DAMAGE_PARENT => {
                actor = cx.actors.get(actor).parent;
                continue;
            }
            team::DAMAGE_SELF | team::DAMAGE_ARMOR => {
                let x = cx.actors.get_mut(actor);
                x.hit_points = x.hit_points.abs() - amount;
                if x.hit_points <= 0 {
                    super::enm::on_actor_destroyed(cx, source, actor);
                    cx.actors.get_mut(actor).hit_points = 0;
                    destroy(cx, actor, true, true);
                    return 0;
                }
                let x = cx.actors.get_mut(actor);
                if x.hit_age == 0 {
                    x.hit_age = x.age;
                }
                cx.play_sound(super::sounds::DAMAGE);
                cx.actors.flash(actor);
                return 0;
            }
            _ => {
                // kDamageGuard.
                cx.play_sound(super::sounds::NO_DAMAGE);
                return 0;
            }
        }
    }
}

/// Actor::Move (0x4085c0): runs the control and moves the actor and its
/// children; false when it is to be destroyed.
pub fn move_actor(cx: &mut Cx, a: Aid) -> bool {
    if cx.actors.get(a).flags & flags::DEBRIS != 0 {
        return false;
    }
    if cx.actors.get(a).control {
        let id = cx.actors.get(a).id;
        if !controls::on_move(cx, id, a) {
            return false;
        }
        let x = cx.actors.get_mut(a);
        x.age = x.age.wrapping_add(1);
    } else {
        let x = cx.actors.get_mut(a);
        if x.age != 0 {
            x.age -= 1;
            if x.age == 0 {
                return false;
            }
        }
    }
    let t = cx.actors.get(a).team;
    if t & (team::HITS | team::HIT_BY_OTHERS) != 0 && cx.actors.flags & 1 != 0
        && let Some(other) = cx.actors.hit_test(a)
            && cx.actors.get(other).team & team::DAMAGE_MASK != 0 {
                let hp = cx.actors.get(a).hit_points.abs();
                if damage(cx, other, hp, a) != 0 || (t & (team::HITS | team::HIT_BY_OTHERS)) == team::HITS {
                    return false;
                }
            }
    let bounds = cx.ctl.boss_bounds;
    let (left, top, right, bottom) = (cx.actors.left, cx.actors.top, cx.actors.right, cx.actors.bottom);
    {
        let x = cx.actors.get_mut(a);
        if x.flags & flags::FACE_START != 0 {
            x.angle = x.start_angle as i16;
        }
        if x.matrix_angle != x.angle {
            let scaled = x.scale != 1.0;
            ActorSystem::rotate(x, scaled);
            let f = x.flags;
            let t = x.tf_mut();
            if f & flags::MIRROR_X != 0 {
                t.m[0][0] = -t.m[0][0];
                t.m[0][1] = -t.m[0][1];
            }
            if f & flags::MIRROR_Y != 0 {
                t.m[1][0] = -t.m[1][0];
                t.m[1][1] = -t.m[1][1];
            }
            x.flags |= flags::MOVED;
            x.matrix_angle = x.angle;
        }
        if x.acceleration != 0.0 {
            x.speed += x.acceleration;
            if x.speed <= 0.0 {
                x.acceleration = 0.0;
                x.speed = 0.0;
                if x.flags & flags::STOP_DIES != 0 {
                    return false;
                }
            }
        }
        if x.speed > 0.0 {
            let sa = x.start_angle as i32;
            let speed = x.speed;
            let relative = x.relative_tf;
            let bounded = x.bounded;
            let t = x.tf_mut();
            t.m[2][0] += sin(sa) * speed;
            t.m[2][1] -= cos(sa) * speed;
            x.flags |= flags::MOVED;
            if !relative {
                let t = &mut x.screen;
                if bounded {
                    if t.m[2][0] < bounds.left {
                        t.m[2][0] = bounds.left;
                    }
                    if t.m[2][0] > bounds.right {
                        t.m[2][0] = bounds.right;
                    }
                    if t.m[2][1] < bounds.top {
                        t.m[2][1] = bounds.top;
                    }
                    if t.m[2][1] > bounds.bottom {
                        t.m[2][1] = bounds.bottom;
                    }
                } else if t.m[2][0] < left || t.m[2][0] > right || t.m[2][1] < top || t.m[2][1] > bottom {
                    x.debris = 0;
                    return false;
                }
            }
        }
    }
    let parent = cx.actors.get(a).parent;
    let pflags = cx.actors.get(parent).flags;
    let pscreen = cx.actors.get(parent).screen;
    {
        let x = cx.actors.get_mut(a);
        x.flags |= pflags & flags::MOVED;
        if x.flags & (flags::MOVED | flags::CHILD) == (flags::MOVED | flags::CHILD) {
            x.screen = Transform::multiply(&pscreen, &x.relative);
        }
    }
    let mut child = cx.actors.get(a).first_child;
    while child != NIL {
        let next = cx.actors.get(child).prev_sibling;
        if !move_actor(cx, child) {
            destroy(cx, child, true, true);
        }
        child = next;
    }
    let x = cx.actors.get_mut(a);
    x.flags = (x.flags & !flags::MOVED) | flags::MOVED_ONCE;
    true
}

/// ActorSystem::MoveAll (0x408c20).
pub fn move_all(cx: &mut Cx) {
    cx.actors.a[0].low_hit_points = (cx.app.frame_count & 0x42 == 0) as i32;
    cx.actors.last_dead = 0;
    let mut actor = cx.actors.get(0).first_child;
    while actor != NIL {
        let next = cx.actors.get(actor).prev_sibling;
        if !move_actor(cx, actor) {
            destroy(cx, actor, true, true);
        }
        actor = next;
    }
    let last = cx.actors.last_dead;
    for i in 0..=last {
        let slot = &mut cx.actors.a[1 + i as usize];
        if slot.flags & flags::DEAD != 0 {
            slot.flags = 0;
        }
    }
}

/// Draws `obj` under `t` (HLSObject::Draw), stepping animations while the
/// time moves.
pub fn draw_obj(cx: &mut Cx, obj: Obj, t: &Transform, color: u32, blend: Blend) {
    match obj {
        Obj::Null => {}
        Obj::Pat(p) => draw_pat(cx, p, t, color, blend),
        Obj::Anm(i) => {
            if let Some(p) = cx.actors.anims[i as usize].object {
                draw_pat(cx, p, t, color, blend);
            }
            if cx.app.time_moves {
                cx.actors.anims[i as usize].step();
            }
        }
        Obj::Shockwave => {
            if color & 0xff00_0000 == 0 {
                return;
            }
            cx.draw.sprite(cx.art.shapes.shockwave, t, Argb(color), blend);
        }
        Obj::Laser => {
            if color & 0xff00_0000 == 0 {
                return;
            }
            let dim = ((color >> 3) & 0x1f00_0000) | (color & 0xff_ffff);
            let pts = cx.ctl.laser.points;
            cx.draw.shape(Primitive::TriangleStrip, t, &pts, Argb(dim), blend);
            cx.draw.shape(Primitive::LineStrip, t, &pts, Argb(color), blend);
        }
        // HLSTxt::Draw (0x40b4d0): prints at the transform's position.
        Obj::Text(i) => {
            if let Some(text) = cx.obj_texts.get(i as usize) {
                let font = text.font.get(&cx.fonts);
                font.print(&mut cx.draw, t.m[2][0], t.m[2][1], &text.text, None, Argb(color), -1, blend);
            }
        }
    }
}

/// HLSPat::Draw (0x409e20).
fn draw_pat(cx: &mut Cx, p: PatId, t: &Transform, color: u32, blend: Blend) {
    if let Some(info) = cx.pats.info(p) {
        if info.empty {
            return;
        }
        let info = *info;
        match (info.sprite, info.clip) {
            (Some(sprite), Some(clip)) => cx.draw.sprite_clipped(sprite, t, clip, Argb(color), blend),
            (Some(sprite), None) => cx.draw.sprite(sprite, t, Argb(color), blend),
            _ => {}
        }
        if let Some(overlay) = info.overlay {
            cx.draw.sprite(overlay, t, Argb(color), blend);
        }
    }
}

/// Marks the hit image with the object's box (HLSObject::Hit).
fn hit_obj(cx: &mut Cx, obj: Obj, x: i32, y: i32, id: u16) {
    let p = match obj {
        Obj::Pat(p) => Some(p),
        Obj::Anm(i) => cx.actors.anims[i as usize].object,
        _ => None,
    };
    if let Some(info) = p.and_then(|p| cx.pats.info(p)).copied() {
        cx.actors.hit.fill(x, y, info.hit[0] as i32, info.hit[1] as i32, id);
    }
}

/// Actor::Draw (0x407f90).
fn draw_actor(cx: &mut Cx, a: Aid) -> Aid {
    let x = cx.actors.get(a);
    let (obj, screen, color, f, hp, low) = (x.sprite, x.screen, x.color, x.flags, x.hit_points, x.low_hit_points);
    // Display only: the draws of one actor carry a key that stays the same
    // while it lives (its slot and the frame it was made on), so the
    // renderer can interpolate them between ticks.
    let born = cx.app.frame_count.wrapping_sub(x.age as u32);
    cx.draw.set_key(((a as u32 + 1).wrapping_mul(0x9e37_79b1) ^ born.wrapping_mul(0x85eb_ca6b)) & 0x3ff_ffff | 1);
    if f & flags::ADDITIVE != 0 {
        draw_obj(cx, obj, &screen, color, Blend::Additive);
    } else if hp < 0 {
        draw_obj(cx, obj, &screen, color, Blend::Normal);
        draw_obj(cx, obj, &screen, 0xff80_ffff, Blend::Additive);
        cx.actors.get_mut(a).hit_points = hp.abs();
    } else if hp <= low && cx.actors.a[0].low_hit_points != 0 {
        draw_obj(cx, obj, &screen, 0xffff_4000, Blend::Normal);
    } else {
        draw_obj(cx, obj, &screen, color, Blend::Normal);
    }
    let x = cx.actors.get(a);
    if x.team & team::HITTABLE != 0 {
        let (sx, sy, id) = (math::ftol(x.sx()), math::ftol(x.sy()), x.hit_id);
        hit_obj(cx, obj, sx, sy, id);
    }
    cx.draw.set_key(0);
    cx.actors.get(a).draw_next
}

/// ActorSystem::DrawAll (0x4081f0).
pub fn draw_all(cx: &mut Cx) {
    draw_all_moved(cx, &[]);
}

/// DrawAll with the draws of some actors moved by an offset (display
/// only: their positions and the hit image are left as they are).
pub fn draw_all_moved(cx: &mut Cx, moved: &[(Aid, [f32; 2])]) {
    cx.actors.hit.clear();
    for layer in 0..ActorSystem::LAYERS {
        let mut a = cx.actors.layers[layer];
        while a != NIL {
            let offset = moved.iter().find(|m| m.0 == a).map(|m| m.1);
            if let Some(o) = offset {
                cx.draw.set_origin(o);
            }
            let next = draw_actor(cx, a);
            if offset.is_some() {
                cx.draw.set_origin([0.0, 0.0]);
            }
            a = next;
        }
    }
}
