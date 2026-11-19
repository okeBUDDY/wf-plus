//! The enemy parameters (CEnmParam, enm_param.cpp): the boss generator.
//! Every part the player destroys is logged with how it went; the next
//! boss is grown from that log on the part templates of plan.dat
//! (EnmPlan, enm_plan.cpp).

pub mod battle;
mod tactics;
pub mod title;

use super::Cx;
use super::actor::Aid;
use super::math::Rand;
use bytemuck::{Pod, Zeroable};

/// CEnmParam::Codes.
pub const CODE_MASK: u32 = 0x3fff_ffff;
pub const SIDE_MASK: u32 = 0xc000_0000;

/// The code of the first child of the part at `code` (ChildBase).
pub fn child_base(code: u32) -> u32 {
    (code & SIDE_MASK) | (code << 3)
}

/// The code of the parent of the part at `code` (ParentCode).
pub fn parent_code(code: u32) -> u32 {
    (code & SIDE_MASK) | ((code & CODE_MASK) >> 3)
}

/// The side of a part code: 0 left (0x40000000), 1 right (0x80000000).
pub fn side_of(code: u32) -> usize {
    ((code & SIDE_MASK) == 0x8000_0000) as usize
}

/// A weapon or extra of plan.dat (EnmPlanItem, 0x1c bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct EnmPlanItem {
    pub code: u32,
    pub flags: u32,
    pub kind: u32,
    pub x: i16,
    pub y: i16,
    pub unknown_10: i16,
    pub low: i16,
    pub high: i16,
    pub debris: i16,
    pub pat_file: i16,
    pub pat: i16,
}

/// A part template of plan.dat (EnmPlanPart, 0x48 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct EnmPlanPart {
    pub item: EnmPlanItem,
    pub unknown_1c: [i16; 6],
    pub score: i32,
    pub hit_points: i16,
    pub x1: i16,
    pub y1: i16,
    pub unknown_32: i16,
    pub lerp_from: i16,
    pub lerp_to: i16,
    pub child_count: i16,
    pub option_count: i16,
    pub option4_count: i16,
    pub extra_child_count: i16,
    pub item_count: i16,
    pub unknown_42: i16,
    /// An index into the items.
    pub items: u32,
}

/// EnmPlanPart::Flags.
pub mod part_flags {
    pub const MIRROR_X: u32 = 0x1;
    pub const MIRROR_Y: u32 = 0x2;
    pub const GUARD: u32 = 0x4;
    pub const AIM_HIGH: u32 = 0x40;
    pub const TURRET: u32 = 0x80;
}

/// Which template a slot uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartRef {
    Root,
    Part(u16),
    /// A weapon: an EnmPlanItem read as an EnmPlanPart (the original casts;
    /// the fields past the item come from the items that follow it).
    Weapon(u16),
}

/// The contents of plan.dat (EnmPlan).
#[derive(Default)]
pub struct EnmPlan {
    pub root: EnmPlanPart,
    pub parts: Vec<EnmPlanPart>,
    pub weapons: Vec<EnmPlanPart>,
    pub items: Vec<EnmPlanItem>,
    /// Parts counted by kind (kind_counts_).
    pub kind_counts: [i32; 6],
}

pub const WEAPON_KIND: u32 = 0x800_0001;

impl EnmPlan {
    /// EnmPlan::Load (0x41b790).
    pub fn load(bytes: &[u8]) -> Option<Self> {
        let i32_at = |at: usize| bytes.get(at..at + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()));
        let part_count = i32_at(4)?.max(0) as usize;
        let weapon_count = i32_at(8)?.max(0) as usize;
        let item_count = i32_at(12)?.max(0) as usize;
        let read = |at: usize, size: usize| -> Vec<u8> {
            let mut v = bytes.get(at..).unwrap_or(&[]).iter().take(size).copied().collect::<Vec<u8>>();
            v.resize(size, 0);
            v
        };
        let root: EnmPlanPart = bytemuck::pod_read_unaligned(&read(16, 0x48));
        let mut at = 0x58;
        let items: Vec<EnmPlanItem> = (0..item_count).map(|i| bytemuck::pod_read_unaligned(&read(at + i * 0x1c, 0x1c))).collect();
        at += item_count * 0x1c;
        let weapon_bytes = read(at, weapon_count * 0x1c + 0x48);
        let weapons = (0..weapon_count).map(|i| bytemuck::pod_read_unaligned(&weapon_bytes[i * 0x1c..i * 0x1c + 0x48])).collect();
        at += weapon_count * 0x1c;
        let mut parts = Vec::with_capacity(part_count);
        let mut kind_counts = [0; 6];
        for i in 0..part_count {
            let p: EnmPlanPart = bytemuck::pod_read_unaligned(&read(at + i * 0x48, 0x48));
            match p.item.kind {
                0x201_0001 | 0x201_0002 => kind_counts[0] += 1,
                0x202_0001 | 0x202_0002 => kind_counts[1] += 1,
                0x401_0001 => kind_counts[3] += 1,
                0x402_0001 => kind_counts[4] += 1,
                0x404_0001 => kind_counts[5] += 1,
                _ => {}
            }
            parts.push(p);
        }
        kind_counts[2] = kind_counts[0] + kind_counts[1];
        Some(EnmPlan { root, parts, weapons, items, kind_counts })
    }

    /// EnmPlan::Find (0x41b6f0).
    pub fn find(&self, code: u32, kind: u32) -> Option<PartRef> {
        let code = code & CODE_MASK;
        if code == 0 {
            return Some(PartRef::Root);
        }
        if kind != 0 && kind != WEAPON_KIND {
            return self.parts.iter().rposition(|p| p.item.code == code && p.item.kind == kind).map(|i| PartRef::Part(i as u16));
        }
        if kind == 0 {
            if let Some(i) = self.parts.iter().rposition(|p| p.item.code == code) {
                return Some(PartRef::Part(i as u16));
            }
        }
        self.weapons.iter().rposition(|w| w.item.code == code).map(|i| PartRef::Weapon(i as u16))
    }

    pub fn get(&self, r: PartRef) -> &EnmPlanPart {
        match r {
            PartRef::Root => &self.root,
            PartRef::Part(i) => &self.parts[i as usize],
            PartRef::Weapon(i) => &self.weapons[i as usize],
        }
    }
}

/// A movement program of the boss (EnmList).
#[derive(Clone, Copy, Debug, Default)]
pub struct EnmList {
    pub arg: i32,
    pub time: i32,
    pub op: i32,
    pub step: i32,
}

impl EnmList {
    pub fn start(&mut self, op: i32, arg: i32) {
        self.op = op;
        self.arg = arg;
        self.step = 0;
        self.time = 0;
    }
    /// Waits `frames` (time counts down), then the next step.
    pub fn wait(&mut self, frames: i32) {
        self.time = frames;
        self.step += 1;
    }
    pub fn set_arg_and_next(&mut self, arg: i32) {
        self.arg = arg;
        self.step += 1;
    }
}

/// A part of the boss being planned (EnmSlot, 0x20 bytes; the first 0x18
/// go into replays).
#[derive(Clone, Copy, Debug, Default)]
pub struct EnmSlot {
    pub code: u32,
    pub kind: u32,
    pub flags: u32,
    pub depth: i16,
    pub low: i16,
    pub high: i16,
    pub hits: i16,
    pub weapon: i8,
    pub level: i8,
    pub power: i16,
    pub part: Option<PartRef>,
    pub lerp_at: i16,
}

/// The first 0x18 bytes of an EnmSlot, as replays store them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct SlotRecord {
    pub code: u32,
    pub kind: u32,
    pub flags: u32,
    pub depth: i16,
    pub low: i16,
    pub high: i16,
    pub hits: i16,
    pub weapon: i8,
    pub level: i8,
    pub power: i16,
}

/// A logged destruction (EnmDestroyRecord).
#[derive(Clone, Copy, Debug, Default)]
pub struct DestroyRecord {
    pub slot: u16,
    pub chain: i16,
    pub rapid: i16,
}

/// Adds `delta`, keeping the value within 0..0x400 (AddClamped).
pub fn add_clamped(value: &mut i16, delta: i32) {
    *value = value.wrapping_add(delta as i16);
    if *value > 0x400 {
        *value = 0x400;
    } else if *value < 0 {
        *value = 0;
    }
}

/// Destructions counted for one side of the boss (EnmSideStats, 10 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct EnmSideStats {
    pub mode: i16,
    pub unknown_02: i16,
    pub unknown_04: i16,
    pub progress: i16,
    pub unknown_08: i16,
}

impl EnmSideStats {
    pub fn score(&self) -> i32 {
        (if self.mode == 0 { self.unknown_04 as i32 } else { self.unknown_08 as i32 + self.unknown_04 as i32 }) + self.unknown_02 as i32
    }
}

/// The title of a boss (EnmTitle, 8 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable, PartialEq, Eq)]
pub struct EnmTitle {
    pub words: [i8; 2],
    pub weapon: i16,
    pub level: i16,
    pub variant: i16,
}

/// The state of CEnmParam saved with a game (EnmState, 0x7c bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct EnmState {
    pub flags: u32,
    pub sides: [EnmSideStats; 2],
    pub color: u32,
    pub unknown_24: i16,
    pub unknown_26: i16,
    pub unknown_28: i16,
    pub unknown_2a: i16,
    pub max_depth: i16,
    pub unknown_2e: i16,
    pub weapon_mask: u32,
    pub weapon_count: i16,
    pub weapons: [i8; 8],
    pub weapon_levels: [i16; 7],
    pub step_mask: u32,
    pub step_count: i16,
    pub step_total: i16,
    pub steps: [[i8; 2]; 19],
    pub title: EnmTitle,
    pub unknown_82: i16,
}

const _: () = assert!(std::mem::size_of::<EnmState>() == 0x7c);

/// The order new weapons are added in, by how the player mostly destroyed
/// parts (0x45d964).
const WEAPON_ORDER: [[i32; 7]; 3] = [[0, 2, 1, 4, 3, 6, 5], [0, 1, 3, 2, 4, 6, 5], [0, 3, 1, 6, 2, 4, 5]];

/// The length of each step; from 9 on that of step 9 (0x45d990).
const STEP_COSTS: [i8; 12] = [1, 1, 1, 1, 1, 1, 3, 1, 3, 5, 0, 0];

/// CEnmParam (g_enm_param).
pub struct EnmParam {
    pub state: EnmState,
    pub turn_speed: i32,
    pub fire_rate: i32,
    pub fire_wait: [i32; 2],
    pub turret_count: i32,
    pub unknown_98: i32,
    pub list_9c: EnmList,
    pub list_b0: EnmList,
    pub lists_c4: [EnmList; 2],
    pub parts_alive: [i32; 2],
    pub unknown_f4: [i32; 2],
    pub armored_parts: [i32; 2],
    pub unknown_104: [i32; 2],
    /// The union at 0x10c: weapon_parts_[7], power_total_, part_total_
    /// while planning; core_power_[2][7] in battle.
    pub u10c: [i32; 14],
    pub core_count: i32,
    pub core_total: i32,
    pub player_dir: i32,
    pub glow_time: i32,
    pub trans_count: i32,
    pub unknown_158: i32,
    pub flare_time: i32,
    pub flare_size: f32,
    pub unknown_164: i32,
    pub score_base: i32,
    pub score_rapid: i32,
    pub score_chain: i32,
    pub chain_count: i32,
    pub rapid_count: i32,
    pub unknown_17c: i32,
    pub slots: Vec<EnmSlot>,
    pub slot_count: i32,
    pub records: Vec<DestroyRecord>,
    pub record_count: usize,
    pub plan: EnmPlan,
    pub list_218: EnmList,
    pub titles: [EnmTitle; 32],
    pub title_count: i32,
    pub deaths: i32,
    pub battle_time: i32,
    pub unknown_338: [i32; 3],
    pub unknown_344: i32,
    pub player_travel: f32,
    pub player_pos: [f32; 2],
    pub player_samples: i32,
    pub new_weapon: i32,
    pub unknown_35c: i32,
}

pub const SLOT_CAPACITY: usize = 0x80;
pub const RECORD_CAPACITY: usize = 0x101;

impl EnmParam {
    /// CEnmParam::Initialize (0x41ab20) with the plan loaded from `plan`.
    pub fn new(plan: EnmPlan) -> Self {
        EnmParam {
            state: EnmState::default(),
            turn_speed: 0,
            fire_rate: 0,
            fire_wait: [0; 2],
            turret_count: 0,
            unknown_98: 0,
            list_9c: EnmList::default(),
            list_b0: EnmList::default(),
            lists_c4: [EnmList::default(); 2],
            parts_alive: [0; 2],
            unknown_f4: [0; 2],
            armored_parts: [0; 2],
            unknown_104: [0; 2],
            u10c: [0; 14],
            core_count: 0,
            core_total: 0,
            player_dir: 0,
            glow_time: 0,
            trans_count: 0,
            unknown_158: 0,
            flare_time: 0,
            flare_size: 0.0,
            unknown_164: 0,
            score_base: 0,
            score_rapid: 0,
            score_chain: 0,
            chain_count: 0,
            rapid_count: 0,
            unknown_17c: 0,
            slots: vec![EnmSlot::default(); SLOT_CAPACITY],
            slot_count: 0,
            records: vec![DestroyRecord::default(); RECORD_CAPACITY],
            record_count: 0,
            plan,
            list_218: EnmList::default(),
            titles: [EnmTitle::default(); 32],
            title_count: 0,
            deaths: 0,
            battle_time: 0,
            unknown_338: [0; 3],
            unknown_344: 0,
            player_travel: 0.0,
            player_pos: [0.0; 2],
            player_samples: 0,
            new_weapon: 0,
            unknown_35c: 0,
        }
    }

    pub fn core_power(&self, side: usize, weapon: usize) -> i32 {
        self.u10c[side * 7 + weapon]
    }
    pub fn core_power_mut(&mut self, side: usize, weapon: usize) -> &mut i32 {
        &mut self.u10c[side * 7 + weapon]
    }

    /// The template of slot `s`.
    pub fn part(&self, s: u16) -> &EnmPlanPart {
        self.plan.get(self.slots[s as usize].part.unwrap_or(PartRef::Root))
    }

    /// CEnmParam::FindSlot: the slot of the part at `code` (never slot 0).
    pub fn find_slot(&self, code: u32) -> Option<u16> {
        if code == 0 {
            return None;
        }
        (1..SLOT_CAPACITY).rev().find(|&i| self.slots[i].code == code).map(|i| i as u16)
    }

    /// CEnmParam::Count (0x41a190).
    fn count(&mut self, s: u16, side: usize) {
        let part = *self.part(s);
        let slot = &mut self.slots[s as usize];
        slot.flags |= 2;
        if part.item.flags & 8 != 0 {
            let range = part.lerp_to as i32 - part.lerp_from as i32;
            let v = if range != 0 {
                part.item.low as i32 + (part.item.high as i32 - part.item.low as i32) * (slot.lerp_at as i32 - part.lerp_from as i32) / range
            } else {
                part.item.low as i32
            } as i16;
            slot.low = v;
            slot.high = v;
        } else {
            slot.low = part.item.low;
            slot.high = part.item.high;
        }
        if part.item.flags & 0x100 != 0 {
            self.state.flags |= 0x10000;
            slot.flags |= 8;
        }
        if part.item.flags & 0x20 != 0 {
            self.state.flags |= 0x20000;
        }
        match slot.kind & 0xff_0000 {
            0x1_0000 => self.state.sides[side].unknown_02 += 1,
            0x2_0000 => self.state.sides[side].unknown_04 += 1,
            0x4_0000 => self.state.sides[side].unknown_08 += 1,
            _ => {}
        }
        self.slots[s as usize].hits = 0;
    }

    /// CEnmParam::AllocSlot (0x41a2a0).
    pub fn alloc_slot(&mut self, code: u32, kind: u32) -> Option<u16> {
        if self.slot_count >= SLOT_CAPACITY as i32 {
            return None;
        }
        let i = (1..SLOT_CAPACITY).rev().find(|&i| self.slots[i].code == 0)?;
        let r = self.plan.find(code, kind);
        let part = *self.plan.get(r.unwrap_or(PartRef::Root));
        let mut depth = 0i16;
        let mut c = code & CODE_MASK;
        while c != 0 {
            c = parent_code(c);
            depth += 1;
        }
        let (low, high) = if part.item.kind & 0x600_0000 != 0 { (part.lerp_from, part.lerp_to) } else { (part.item.low, part.item.high) };
        self.slots[i] = EnmSlot { code, part: r, kind: part.item.kind, flags: 0, hits: 0, weapon: 0, level: 0, power: 10, depth, low, high, lerp_at: self.slots[i].lerp_at };
        if depth > self.state.max_depth {
            self.state.max_depth = depth;
        }
        self.slot_count += 1;
        Some(i as u16)
    }

    /// CEnmParam::ChooseWeapon (0x41a3a0).
    fn choose_weapon(&mut self, s: u16, parent: Option<u16>, rng: &mut Rand) {
        let mask = self.part(s).item.flags & self.state.weapon_mask;
        let slot = self.slots[s as usize];
        if mask & (0x10000u32.wrapping_shl(slot.weapon as u32)) != 0 && slot.flags & 4 != 0 {
            self.slots[s as usize].level = 0;
            return;
        }
        self.slots[s as usize].flags &= !4;
        let parent = parent.map(|p| self.slots[p as usize]).unwrap_or_default();
        let weapon;
        let level;
        if mask != 0 {
            let mut w = if slot.weapon < 0 && parent.flags & 4 != 0 {
                parent.weapon as i32
            } else if self.new_weapon >= 0 {
                self.new_weapon
            } else {
                self.state.weapons[(rng.rand() % self.state.weapon_count as i32) as usize] as i32
            };
            while mask & (0x10000u32 << w) == 0 {
                w = rng.rand() % 7;
            }
            weapon = w;
            level = (rng.rand() % 3 + 1) as i8;
        } else {
            weapon = 0;
            level = 1;
        }
        let max = self.state.weapon_levels[weapon as usize] as i32;
        let mut power = slot.power as i32;
        if power < max * 100 / 70 {
            power = max * 100 / 70;
        }
        if power < parent.power as i32 {
            power = parent.power as i32;
        }
        if power < 10 {
            power = 10;
        } else if power > max {
            power = max;
        }
        let slot = &mut self.slots[s as usize];
        slot.level = level;
        slot.weapon = weapon as i8;
        slot.power = power as i16;
    }

    /// CEnmParam::LogDestroy (0x41a4c0).
    pub fn log_destroy(&mut self, s: u16, chain: i32, rapid: i32) {
        if self.record_count < RECORD_CAPACITY {
            self.records[self.record_count] = DestroyRecord { slot: s, chain: chain as i16, rapid: rapid as i16 };
            self.record_count += 1;
            if rapid > 1 {
                let side = &mut self.state.sides[side_of(self.slots[s as usize].code)];
                if side.mode == 0 {
                    side.unknown_08 += 1;
                }
                add_clamped(&mut self.state.unknown_26, rapid * 7);
                add_clamped(&mut self.state.unknown_2a, rapid * -5);
            }
            if chain == 1 {
                add_clamped(&mut self.state.unknown_26, rapid);
            }
        }
    }

    /// CEnmParam::Save (0x41a5b0): the slot count, the state and the slots
    /// from the top down.
    pub fn save(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.slot_count.to_le_bytes());
        out.extend_from_slice(bytemuck::bytes_of(&self.state));
        for i in 0..self.slot_count.max(0) as usize {
            let s = &self.slots[SLOT_CAPACITY - 1 - i];
            let r = SlotRecord { code: s.code, kind: s.kind, flags: s.flags, depth: s.depth, low: s.low, high: s.high, hits: s.hits, weapon: s.weapon, level: s.level, power: s.power };
            out.extend_from_slice(bytemuck::bytes_of(&r));
        }
    }

    /// CEnmParam::Load (0x41a660); false on a short or bad record.
    pub fn load(&mut self, r: &mut super::replay::Reader) -> bool {
        let Some(count) = r.i32() else { return false };
        self.slot_count = count;
        let Some(state) = r.pod::<EnmState>() else { return false };
        self.state = state;
        for i in 0..count.max(0) as usize {
            if i >= SLOT_CAPACITY {
                return false;
            }
            let Some(rec) = r.pod::<SlotRecord>() else { return false };
            let part = self.plan.find(rec.code, rec.kind);
            if part.is_none() {
                return false;
            }
            let s = &mut self.slots[SLOT_CAPACITY - 1 - i];
            *s = EnmSlot { code: rec.code, kind: rec.kind, flags: rec.flags, depth: rec.depth, low: rec.low, high: rec.high, hits: rec.hits, weapon: rec.weapon, level: rec.level, power: rec.power, part, lerp_at: 0 };
        }
        true
    }

    /// CEnmParam::Grow (0x41a820).
    fn grow(&mut self, parent: Option<u16>, code: u32, kind: u32, rng: &mut Rand) {
        let s = match self.find_slot(code) {
            None => {
                let Some(s) = self.alloc_slot(code, kind) else { return };
                if self.slots[s as usize].kind & 0x800_0000 == 0 {
                    self.slots[s as usize].flags = 1;
                }
                self.slots[s as usize].weapon = -1;
                self.choose_weapon(s, parent, rng);
                s
            }
            Some(s) if self.slots[s as usize].flags & 2 == 0 => {
                let slot = &mut self.slots[s as usize];
                if slot.level > 0 {
                    slot.level -= 1;
                    if slot.level == 0 {
                        self.choose_weapon(s, parent, rng);
                    }
                }
                s
            }
            Some(s) => {
                let slot = self.slots[s as usize];
                if self.state.flags & 0x10000 != 0 && slot.flags & 8 == 0 {
                    let limit = if slot.kind & 0x4_0000 != 0 {
                        3
                    } else if slot.kind & 0x200_0000 != 0 {
                        5
                    } else {
                        4
                    };
                    if slot.hits > limit {
                        self.slots[s as usize].flags |= 8;
                    }
                }
                let base = (code & SIDE_MASK) | (code << 3);
                let part = *self.part(s);
                let count = part.extra_child_count as i32 + part.child_count as i32;
                for i in 0..count {
                    self.grow(Some(s), base | i as u32, 0, rng);
                }
                let mut index = count as u32;
                let mode = self.state.sides[side_of(base)].mode;
                let kind = self.slots[s as usize].kind;
                if mode & 1 != 0 && kind & 0x1_0000 != 0 {
                    for _ in 0..part.option_count {
                        self.grow(Some(s), base | index, 0x401_0001, rng);
                        index += 1;
                    }
                    self.slots[s as usize].flags |= 0x1000;
                }
                if mode & 2 != 0 && kind & 0x2_0000 != 0 {
                    for _ in 0..part.option_count {
                        self.grow(Some(s), base | index, 0x402_0001, rng);
                        index += 1;
                    }
                    self.slots[s as usize].flags |= 0x2000;
                }
                if mode & 4 != 0 {
                    index = index.wrapping_add(part.option_count as u32);
                    for _ in 0..part.option4_count {
                        self.grow(Some(s), base | index, 0x404_0001, rng);
                        index += 1;
                    }
                    self.slots[s as usize].flags |= 0x4000;
                }
                return;
            }
        };
        let slot = self.slots[s as usize];
        self.u10c[slot.weapon as usize] += 1;
        self.u10c[8] += 1;
        self.u10c[7] += slot.power as i32;
    }

    /// CEnmParam::CountDestroyed (0x41a730).
    fn count_destroyed(&mut self, mut count: i32, kinds: &mut [u32; 2], unique: bool, all: bool) -> i32 {
        for i in 0..self.record_count {
            let rec = self.records[i];
            let slot = self.slots[rec.slot as usize];
            if slot.flags & 3 == 1 && (all || rec.chain <= 2) {
                let side = match slot.code & SIDE_MASK {
                    0x4000_0000 => 0,
                    0x8000_0000 => 1,
                    _ => continue,
                };
                if unique {
                    if kinds[side] == 0 {
                        self.count(rec.slot, side);
                        kinds[side] = self.slots[rec.slot as usize].kind;
                        count -= 1;
                        if kinds[0] != 0 && kinds[1] != 0 {
                            return count;
                        }
                    }
                } else {
                    self.count(rec.slot, side);
                    if kinds[side] == 0 {
                        kinds[side] = self.slots[rec.slot as usize].kind;
                    }
                    count -= 1;
                }
                if count <= 0 {
                    return count;
                }
            }
        }
        count
    }

    /// AddStep: adds step `id` unless the game has it already.
    fn add_step(&mut self, id: i32) {
        let count = self.state.step_count as usize;
        if self.state.step_mask & (1 << id) == 0 {
            self.state.steps[count][0] = id as i8;
            let cost = STEP_COSTS[if id < 9 { id as usize } else { 9 }];
            self.state.steps[count][1] = if count == 0 { cost } else { self.state.steps[count - 1][1].wrapping_add(cost) };
            self.state.step_count += 1;
            self.state.step_total += cost as i16;
            self.state.step_mask |= 1 << id;
        }
    }

    /// CEnmParam::Plan (0x41abc0): plans the next boss; `score` is what the
    /// last stage scored, `stage` the stage it was (g_global.stage).
    pub fn plan(&mut self, score: u32, stage: i32, rng: &mut Rand) {
        if self.player_samples < 1 {
            self.player_samples = 1;
        }
        if self.battle_time < 1 {
            self.battle_time = 1;
        }
        let count = (score / 300_000) as i32 + 2;
        let mut add = super::math::ftol(self.player_travel / self.player_samples as f32);
        let rate = self.unknown_344 as f32 / self.battle_time as f32;
        if add >= 50 {
            add = 50;
        }
        add_clamped(&mut self.state.unknown_28, add);
        add_clamped(&mut self.state.unknown_2a, super::math::ftol(rate * 100.0));
        self.state.unknown_24 = self.state.unknown_24.wrapping_add((self.state.unknown_26 as i32 * 70 / 1024 + 10) as i16);
        if self.state.unknown_24 > 1200 {
            self.state.unknown_24 = 1200;
        }

        let mut kinds = [0u32; 2];
        let mut left = self.count_destroyed(count, &mut kinds, true, false);
        self.new_weapon = -1;
        let wc = self.state.weapon_count as i32;
        if wc < 6 && (stage <= 1 || (left > 0 && stage >= wc * 2 - 2)) {
            let u = self.unknown_338;
            let order = if u[0] > u[1] && u[0] > u[2] {
                0
            } else if u[1] > u[0] && u[1] > u[2] {
                1
            } else if u[2] > u[0] && u[2] > u[1] {
                2
            } else {
                rng.rand() % 3
            };
            for &weapon in &WEAPON_ORDER[order as usize] {
                let bit = 0x10000u32 << weapon;
                if self.state.weapon_mask & bit == 0 {
                    self.state.weapon_mask |= bit;
                    self.state.weapons[self.state.weapon_count as usize] = weapon as i8;
                    self.state.weapon_count += 1;
                    self.new_weapon = weapon;
                    break;
                }
            }
            left -= 1;
        }

        if self.state.sides[0].mode != 0 && self.state.sides[1].mode != 0 {
            if left > 2 {
                left = 2;
            }
            if left > 0 {
                left = self.count_destroyed(left, &mut kinds, false, false);
                if left > 0 {
                    self.count_destroyed(left, &mut kinds, true, true);
                }
            }
        }
        let kc = self.plan.kind_counts;
        for i in 0..2 {
            let side = &mut self.state.sides[i];
            if side.mode == 0 {
                if side.unknown_08 as i32 >= kc[2] {
                    side.mode = 4;
                    side.unknown_08 = 0;
                } else {
                    match kinds[i] & 0xff_0000 {
                        0x1_0000 => {
                            if side.unknown_02 as i32 >= kc[0] {
                                side.mode = 1;
                                side.unknown_08 = 0;
                            } else if side.unknown_04 as i32 >= kc[1] {
                                side.mode = 2;
                                side.unknown_08 = 0;
                            }
                        }
                        0x2_0000 => {
                            if side.unknown_04 as i32 >= kc[1] {
                                side.mode = 2;
                                side.unknown_08 = 0;
                            } else if side.unknown_02 as i32 >= kc[0] {
                                side.mode = 1;
                                side.unknown_08 = 0;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if self.state.sides[0].progress >= 100 && self.state.sides[1].progress >= 100 {
            for side in &mut self.state.sides {
                if side.mode > 0 && side.mode <= 2 {
                    side.mode = 3;
                }
            }
        }
        for side in &mut self.state.sides {
            let (n, d) = match side.mode {
                1 => (side.unknown_02 as i32, kc[0] + kc[3]),
                2 => (side.unknown_04 as i32, kc[1] + kc[4]),
                3 => (side.unknown_08 as i32 + side.unknown_04 as i32 + side.unknown_02 as i32, kc[5] + kc[1] + kc[0]),
                4 => (side.unknown_02 as i32 + side.unknown_04 as i32, kc[0] + kc[1] + kc[3] + kc[4]),
                _ => (0, 1),
            };
            side.progress = if d != 0 { (n * 100 / d) as i16 } else { 0 };
        }

        if stage >= 1 {
            self.add_step(1);
        }
        if self.state.flags & 0x10000 != 0 {
            self.add_step(6);
        }
        let mut mode0 = self.state.sides[0].mode;
        let mut mode1 = self.state.sides[1].mode;
        if self.state.sides[0].progress < 65 {
            mode0 = 0;
        }
        if self.state.sides[1].progress < 65 {
            mode1 = 0;
        }
        if self.state.flags & 0x20000 != 0 && (mode0 != 0 || mode1 != 0) {
            self.add_step(8);
        }
        if mode0 & 1 != 0 {
            self.add_step(2);
            if mode1 & 1 != 0 {
                self.add_step(9);
            }
            if mode1 & 2 != 0 {
                self.add_step(10);
            }
            if mode1 & 4 != 0 {
                self.add_step(11);
            }
        }
        if mode0 & 2 != 0 {
            self.add_step(4);
            self.add_step(5);
            if mode1 & 1 != 0 {
                self.add_step(12);
            }
            if mode1 & 2 != 0 {
                self.add_step(13);
            }
            if mode1 & 4 != 0 {
                self.add_step(14);
            }
        }
        if mode0 & 4 != 0 {
            if mode1 & 1 != 0 {
                self.add_step(15);
            }
            if mode1 & 2 != 0 {
                self.add_step(16);
            }
            if mode1 & 4 != 0 {
                self.add_step(17);
            }
        }
        if mode1 & 1 != 0 {
            self.add_step(3);
        }
        if mode1 & 4 != 0 {
            self.add_step(7);
        }
        if mode1 & 2 != 0 {
            self.add_step(4);
            self.add_step(5);
        }

        for i in 0..9 {
            self.u10c[i] = 0;
        }
        let root = self.find_slot(0x4000_0000);
        self.grow(root, 0x4000_0001, 0, rng);
        self.grow(root, 0x4000_0002, 0, rng);
        let root = self.find_slot(0x8000_0000);
        self.grow(root, 0x8000_0001, 0, rng);
        self.grow(root, 0x8000_0002, 0, rng);
        self.choose_title(rng);
    }

    /// CEnmParam::Reset (0x41b5b0): back to the first boss.
    pub fn reset(&mut self, rng: &mut Rand) {
        self.state = EnmState::default();
        self.state.color = 0xff00_ff00;
        self.state.unknown_24 = 200;
        self.state.max_depth = 0;
        self.slot_count = 0;
        for s in &mut self.slots {
            s.code = 0;
        }
        self.state.step_mask = 0x40000;
        let count = self.state.step_count as usize;
        self.state.steps[count][0] = 0;
        let step = STEP_COSTS[0];
        self.state.steps[count][1] = if count == 0 { step } else { self.state.steps[count - 1][1] + step };
        self.state.step_total += step as i16;
        self.state.step_count += 1;
        self.state.step_mask |= 1;
        self.alloc_slot(0xc000_0000, 0);
        self.alloc_slot(0x4000_0000, 0);
        self.alloc_slot(0x8000_0000, 0);
        self.title_count = 0;
        self.plan(0, 0, rng);
    }

    /// CEnmParam::GetParams (0x419fa0): the parameters of the results.
    pub fn get_params(&self, out: &mut [i16; 16]) -> usize {
        let count = self.state.weapon_count as usize + 6;
        let kc = self.plan.kind_counts;
        let total = kc[5] + kc[1] + kc[0];
        let side0 = self.state.sides[0].score() * 800 / total.max(1);
        out[0] = (self.state.sides[1].score() * 800 / total.max(1)) as i16;
        out[1] = self.state.unknown_2a;
        out[2] = super::math::ftos((self.state.unknown_24 as i32 - 200) as f32 * 1.5);
        out[3] = side0 as i16;
        out[4] = self.state.unknown_28;
        out[5] = self.state.unknown_26;
        for i in 6..count.min(16) {
            out[i] = self.state.weapon_levels[self.state.weapons[i - 6] as usize];
        }
        count
    }

    /// CEnmParam::FormatResult (0x41a090): the results text, its columns
    /// and lines.
    pub fn format_result(&self, stage: i32, rules: &super::game_state::GameRules) -> (String, i32, i32) {
        let mut t = format!("@@1STAGE {:<3}                       @@0\n\n", stage);
        t += &format!("  BATTLE TIME         {:3}.{:02} SEC\n\n", self.battle_time / 60, self.battle_time % 60);
        let mut count = 2;
        if rules.time_limit > 0 && rules.mode == 0 {
            t += &format!("  DEAD PENALTY  *{:<2}   {:3}.00 SEC\n\n", self.deaths, self.deaths * 20);
            count = 3;
        }
        t += &format!(
            "  BASE POINT        {:8} PTS\n\n  RAPID DESTROY *{:<2} {:8} PTS\n\n  CHAIN DESTROY *{:<2} {:8} PTS\n\n@@1TOTAL ------------- {:8} PTS\n\n",
            self.score_base,
            self.rapid_count,
            self.score_rapid,
            self.chain_count,
            self.score_chain,
            self.score_base + self.score_rapid + self.score_chain
        );
        (t, 32, count + 4)
    }
}

/// CEnmParam::OnActorDestroyed (0x41aa60), Actor::destroy_hook_: logs what
/// destroyed the player.
pub fn on_actor_destroyed(cx: &mut Cx, source: Aid, actor: Aid) {
    if cx.actors.get(actor).id != 1 {
        return;
    }
    let s = cx.actors.get(source).work[0] as u16;
    let enm = &mut cx.enm;
    if s != 0 {
        let slot = &mut enm.slots[s as usize];
        slot.flags |= 4;
        add_clamped(&mut slot.power, 50);
        let (power, weapon) = (slot.power, slot.weapon);
        let level = &mut enm.state.weapon_levels[(weapon as u8 as usize).min(6)];
        if power > *level {
            *level = power;
        }
    }
    add_clamped(&mut enm.state.unknown_26, -50);
    cx.global.entry.deaths += 1;
    enm.deaths += 1;
}
