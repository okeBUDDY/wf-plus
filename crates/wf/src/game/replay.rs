//! Replays (replay.cpp, Replay g_replay): the player's buttons of a stage,
//! recorded as runs of (buttons, frames) after a header that restores the
//! game: the rand() seed, the stage, the score, the clock, the stock, the
//! rules, the boss plan (CEnmParam::Save) and the player (PlayerControl::Save).
//! Versions 101 and 103 of the original play back bit-exact.

use super::Cx;
use super::actor::{Aid, player};
use super::game_state::GameRules;
use bytemuck::Pod;
use std::path::PathBuf;

pub const VERSION: i32 = 103;
pub const OLD_VERSION: i32 = 101;
/// The port's HD recordings: version 103's header, then the arena's
/// width and height (u16 each). Outside 101..=103, so the original's
/// IsReplayFile rejects them cleanly instead of misplaying them.
pub const HD_VERSION: i32 = 1103;

/// Reads little-endian values from a replay.
pub struct Reader {
    data: Vec<u8>,
    at: usize,
}

impl Reader {
    pub fn new(data: Vec<u8>) -> Self {
        Reader { data, at: 0 }
    }
    /// File::ReadExact of a plain struct.
    pub fn pod<T: Pod>(&mut self) -> Option<T> {
        let n = std::mem::size_of::<T>();
        let b = self.data.get(self.at..self.at + n)?;
        self.at += n;
        Some(bytemuck::pod_read_unaligned(b))
    }
    pub fn i32(&mut self) -> Option<i32> {
        self.pod::<i32>()
    }
    pub fn u16(&mut self) -> Option<u16> {
        self.pod::<u16>()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    None,
    Record,
    Play,
}

/// The header after the version (Replay::Header).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Header {
    seed: i32,
    stage: i32,
    score: i32,
    timer: i32,
    lives: u16,
    extend_count: u16,
    rules: GameRules,
}

/// Replay (g_replay).
pub struct Replay {
    pub mode: Mode,
    open: bool,
    path: PathBuf,
    reader: Option<Reader>,
    writer: Option<Vec<u8>>,
    run_keys: u16,
    run_frames: u16,
    seed: i32,
    frame: i32,
    last_keys: u16,
}

impl Default for Replay {
    fn default() -> Self {
        Replay { mode: Mode::None, open: false, path: PathBuf::new(), reader: None, writer: None, run_keys: 0, run_frames: 0, seed: 0, frame: 0, last_keys: 0 }
    }
}

/// Replay::IsReplayFile (0x415ac0).
pub fn is_replay_file(path: &std::path::Path) -> bool {
    match wf_engine::fs::read(path) {
        Ok(b) if b.len() >= 4 => {
            let v = u32::from_le_bytes(b[0..4].try_into().unwrap());
            (OLD_VERSION as u32..=VERSION as u32).contains(&v) || v == HD_VERSION as u32
        }
        _ => false,
    }
}

impl Replay {
    fn close_file(&mut self) {
        if let Some(data) = self.writer.take() {
            if let Some(dir) = self.path.parent() {
                let _ = wf_engine::fs::create_dir_all(dir);
            }
            if let Err(e) = wf_engine::fs::write(&self.path, data) {
                crate::log::warn(&format!("cannot write {}: {e}", self.path.display()));
            }
        }
        self.reader = None;
    }

    /// Replay::Close (0x415590).
    pub fn close(&mut self) {
        if self.open && self.mode == Mode::Record
            && let Some(w) = &mut self.writer {
                w.extend_from_slice(&self.run_keys.to_le_bytes());
                w.extend_from_slice(&self.run_frames.to_le_bytes());
            }
        self.close_file();
        self.open = false;
    }

    /// Replay::Open (0x415720): switches to `mode` on `path`.
    pub fn open(&mut self, mode: Mode, path: PathBuf) {
        self.close();
        self.path = path;
        self.mode = mode;
        self.open = false;
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Replay::GetInput (0x4155c0): the buttons held and pressed this frame.
    pub fn get_input(&mut self, held_pad: u16, pressed_pad: u16) -> (u16, u16) {
        let mut held = held_pad;
        let mut down = pressed_pad;
        match self.mode {
            Mode::Record => {
                if self.open {
                    let w = self.writer.get_or_insert_with(Vec::new);
                    if self.frame == 0 {
                        w.extend_from_slice(&down.to_le_bytes());
                        self.run_keys = held;
                        self.run_frames = 1;
                    } else {
                        if self.last_keys != held {
                            w.extend_from_slice(&self.run_keys.to_le_bytes());
                            w.extend_from_slice(&self.run_frames.to_le_bytes());
                            self.run_keys = held;
                            self.run_frames = 1;
                        } else {
                            self.run_frames = self.run_frames.wrapping_add(1);
                        }
                        down = !self.last_keys & held;
                    }
                    self.frame += 1;
                }
            }
            Mode::Play => {
                down = 0;
                held = 0;
                if self.open {
                    let r = self.reader.as_mut();
                    let mut ok = r.is_some();
                    if let Some(r) = r {
                        if self.frame == 0 {
                            match r.u16() {
                                Some(d) => down = d,
                                None => ok = false,
                            }
                            match (r.u16(), r.u16()) {
                                (Some(k), Some(f)) => {
                                    self.run_keys = k;
                                    self.run_frames = f;
                                }
                                _ => ok = false,
                            }
                            held = self.run_keys;
                            self.run_frames = self.run_frames.wrapping_sub(1);
                        } else {
                            if (self.run_frames as i16) <= 0 {
                                match (r.u16(), r.u16()) {
                                    (Some(k), Some(f)) => {
                                        self.run_keys = k;
                                        self.run_frames = f;
                                    }
                                    _ => ok = false,
                                }
                            }
                            held = self.run_keys;
                            down = !self.last_keys & held;
                            self.run_frames = self.run_frames.wrapping_sub(1);
                        }
                    }
                    if !ok {
                        self.open = false;
                    }
                    self.frame += 1;
                }
            }
            Mode::None => {}
        }
        self.last_keys = held;
        (held, down)
    }
}

/// Replay::LoadHeader (0x415790): restores the game from the replay's
/// header; false when it cannot be read.
pub fn load_header(cx: &mut Cx) -> bool {
    if cx.replay.mode != Mode::Play {
        return true;
    }
    let Ok(data) = wf_engine::fs::read(&cx.replay.path) else { return false };
    let mut r = Reader::new(data);
    let Some(version) = r.i32() else { return false };
    let mut rules = GameRules::default();
    let header;
    let (lives, extend_count): (i32, i32);
    match version {
        OLD_VERSION => {
            let (Some(seed), Some(stage), Some(score), Some(timer)) = (r.i32(), r.i32(), r.i32(), r.i32()) else { return false };
            header = Header { seed, stage, score, timer, ..Default::default() };
            lives = -1;
            extend_count = -1;
        }
        VERSION | HD_VERSION => {
            let Some(h) = r.pod::<Header>() else { return false };
            header = h;
            lives = h.lives as i32;
            extend_count = h.extend_count as i32;
            rules = h.rules;
        }
        _ => return false,
    }
    let arena = if version == HD_VERSION {
        let (Some(w), Some(_h)) = (r.u16(), r.u16()) else { return false };
        super::arena::Arena::hd(w as f32)
    } else {
        super::arena::Arena::CLASSIC
    };
    cx.set_arena(arena);
    cx.replay.seed = header.seed;
    cx.game.start(&rules);
    cx.game.timer = header.timer;
    cx.game.bonus = 0;
    cx.game.set_lives((lives + 1) as i16, extend_count as i16);
    cx.global.entry.score = header.score as u32;
    cx.global.stage = header.stage - 1;
    if !cx.enm.load(&mut r) {
        return false;
    }
    cx.replay.reader = Some(r);
    true
}

/// Replay::LoadPlayer (0x415540).
pub fn load_player(cx: &mut Cx, player: Aid) -> bool {
    if cx.replay.mode != Mode::Play {
        return true;
    }
    let Some(r) = cx.replay.reader.as_mut() else { return false };
    let Some(rec) = r.pod::<player::PlayerRecord>() else {
        cx.replay.reader = None;
        return false;
    };
    player::load(cx, player, &rec);
    true
}

/// Replay::Start (0x415910): starts recording (header and player) or
/// playing back; seeds rand() either way.
pub fn start(cx: &mut Cx, player: Option<Aid>) {
    match cx.replay.mode {
        Mode::Record => {
            cx.replay.close();
            cx.replay.seed = cx.rng.rand();
            let mut w = Vec::new();
            let classic = cx.arena.is_classic();
            w.extend_from_slice(&if classic { VERSION } else { HD_VERSION }.to_le_bytes());
            let h = Header {
                seed: cx.replay.seed,
                stage: cx.global.stage,
                score: cx.global.entry.score as i32,
                timer: cx.game.timer + cx.game.bonus,
                lives: cx.game.lives as u16,
                extend_count: cx.game.extend_count as u16,
                rules: cx.game.rules,
            };
            w.extend_from_slice(bytemuck::bytes_of(&h));
            if !classic {
                for v in cx.arena.size_u16() {
                    w.extend_from_slice(&v.to_le_bytes());
                }
            }
            cx.enm.save(&mut w);
            if let Some(p) = player {
                w.extend_from_slice(bytemuck::bytes_of(&player::save(cx, p)));
            }
            cx.replay.writer = Some(w);
        }
        Mode::Play => {}
        Mode::None => return,
    }
    let seed = cx.replay.seed as u32;
    cx.rng.srand(seed);
    cx.replay.open = true;
    cx.replay.frame = 0;
    cx.replay.run_frames = 0;
}
