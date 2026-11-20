//! .seq music (CSndFileSEQ, snd_file_seq.cpp): a list of commands that
//! plays WAV/OGG pieces one after another, looping and branching on its
//! own variables and on the flags the game sets (g_seq_flags).

use super::{SoundBuffer, ogg, wav};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

const SEQ_VERSION: i32 = 100;

const OP_PLAY: u8 = 0x00;
const OP_PLAY_IF_EQUAL: u8 = 0x01;
const OP_PLAY_IF_NOT_EQUAL: u8 = 0x02;
const OP_PLAY_IF_AT_LEAST: u8 = 0x03;
const OP_PLAY_IF_AT_MOST: u8 = 0x04;
const OP_END: u8 = 0x10;
const OP_REPEAT: u8 = 0x11;
const OP_SET: u8 = 0x20;
const OP_ADD: u8 = 0x21;
const OP_RANDOM: u8 = 0x22;
const OP_JUMP: u8 = 0x30;
const OP_JUMP_IF_EQUAL: u8 = 0x31;
const OP_JUMP_IF_NOT_EQUAL: u8 = 0x32;
const OP_JUMP_IF_AT_LEAST: u8 = 0x33;
const OP_JUMP_IF_AT_MOST: u8 = 0x34;

/// A command (6 bytes).
#[derive(Clone, Copy, Debug)]
struct Command {
    op: u8,
    count: u8,
    var: u8,
    operand: i8,
    /// A name offset or a command index; for OP_RANDOM, low and high.
    target: u16,
}

impl Command {
    fn range(&self) -> (i8, i8) {
        (self.target as u8 as i8, (self.target >> 8) as u8 as i8)
    }
}

/// The flags the game shares with the music (g_seq_flags).
pub type SeqFlags = Arc<[AtomicU8; 8]>;

/// A loaded .seq: its commands and its pieces, decoded.
pub struct SeqData {
    commands: Vec<Command>,
    /// The pieces by name offset.
    pieces: Vec<(u16, Option<Arc<Vec<f32>>>)>,
    pub rate: u32,
}

impl SeqData {
    /// CSndFileSEQ::Open (0x40f1b0): reads the commands and decodes every
    /// piece they name (relative to the .seq's directory). Pieces whose
    /// format differs from the first one are left out, as OpenPiece does.
    pub fn load(path: &Path) -> Result<Self, String> {
        let b = crate::platform::read_file(path).map_err(|e| e.to_string())?;
        let u16_at = |o: usize| b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]));
        if b.len() < 12 || i32::from_le_bytes(b[0..4].try_into().unwrap()) != SEQ_VERSION {
            return Err("not a version 100 .seq".into());
        }
        let count = u16_at(4).unwrap() as usize;
        let names_size = u16_at(6).unwrap() as usize;
        let mut commands = Vec::with_capacity(count);
        let mut at = 12;
        for _ in 0..count {
            let c = b.get(at..at + 6).ok_or("truncated commands")?;
            commands.push(Command { op: c[0], count: c[1], var: c[2], operand: c[3] as i8, target: u16::from_le_bytes([c[4], c[5]]) });
            at += 6;
        }
        let names = b.get(at..at + names_size).ok_or("truncated names")?;
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut pieces: Vec<(u16, Option<Arc<Vec<f32>>>)> = Vec::new();
        let mut rate = 0;
        for c in &commands {
            if c.op > OP_PLAY_IF_AT_MOST || pieces.iter().any(|p| p.0 == c.target) {
                continue;
            }
            let start = c.target as usize;
            let name: Vec<u8> = names.get(start..).unwrap_or(&[]).iter().copied().take_while(|&ch| ch != 0).collect();
            let name = String::from_utf8_lossy(&name).into_owned();
            let file = dir.join(&name);
            let decoded = match name.to_ascii_lowercase().chars().last() {
                Some('v') => crate::platform::read_file(&file).ok().and_then(|d| wav::decode(&d, 1).ok()),
                Some('g') => crate::platform::read_file(&file).ok().and_then(|d| ogg::decode(&d, 1).ok()),
                _ => None,
            };
            let samples = match decoded {
                Some(SoundBuffer { samples, rate: r, .. }) if rate == 0 || rate == r => {
                    rate = r;
                    Some(samples)
                }
                _ => {
                    crate::log::warn(&format!("{}: cannot play the piece {name}", path.display()));
                    None
                }
            };
            pieces.push((c.target, samples));
        }
        if rate == 0 {
            return Err("no playable piece".into());
        }
        Ok(SeqData { commands, pieces, rate })
    }

    fn piece(&self, target: u16) -> Option<Arc<Vec<f32>>> {
        self.pieces.iter().find(|p| p.0 == target).and_then(|p| p.1.clone())
    }
}

/// The playing state of a .seq (the CSndFileSEQ fields).
#[derive(Clone)]
pub struct SeqPlayer {
    data: Arc<SeqData>,
    flags: SeqFlags,
    pc: u16,
    repeat_count: i16,
    piece_loops: i16,
    /// vars_: the original has 4 bytes but indexes 16.
    vars: [i8; 16],
    /// The C runtime's rand() of the streaming thread: its own state,
    /// seeded 1 (the multithreaded CRT keeps one per thread).
    seed: u32,
}

impl SeqPlayer {
    pub fn new(data: Arc<SeqData>, flags: SeqFlags) -> Self {
        SeqPlayer { data, flags, pc: 0, repeat_count: 0, piece_loops: 0, vars: [0; 16], seed: 1 }
    }

    fn rand(&mut self) -> i32 {
        self.seed = self.seed.wrapping_mul(214_013).wrapping_add(2_531_011);
        ((self.seed >> 16) & 0x7fff) as i32
    }

    fn get(&self, id: u8) -> i8 {
        if id & 0xf0 == 0x10 {
            // Past the 8 flags the original reads whatever follows them.
            self.flags.get((id & 0xf) as usize).map_or(0, |f| f.load(Ordering::Relaxed) as i8)
        } else {
            self.vars[(id & 0xf) as usize]
        }
    }

    fn set(&mut self, id: u8, v: i8) {
        if id & 0xf0 == 0x10 {
            if let Some(f) = self.flags.get((id & 0xf) as usize) {
                f.store(v as u8, Ordering::Relaxed);
            }
        } else {
            self.vars[(id & 0xf) as usize] = v;
        }
    }

    /// Seek(0) (0x40f010): back to the first command.
    pub fn rewind(&mut self) {
        self.pc = 0;
        self.piece_loops = 0;
        self.repeat_count = 0;
    }

    fn play(&mut self, c: &Command) -> Option<u16> {
        self.piece_loops = if c.count > 0 { c.count as i16 } else { -1 };
        Some(c.target)
    }

    /// Next (0x40ec40): runs the commands up to the next piece.
    fn next_target(&mut self) -> Option<u16> {
        let commands = self.data.commands.clone();
        if self.piece_loops != 0 {
            let c = commands.get(self.pc as usize)?;
            if self.piece_loops <= 0 {
                return Some(c.target);
            }
            self.piece_loops -= 1;
            if self.piece_loops != 0 {
                return Some(c.target);
            }
            self.pc += 1;
        }
        let mut budget = 1024;
        while (self.pc as usize) < commands.len() && budget > 0 {
            let c = commands[self.pc as usize];
            budget -= 1;
            let v = self.get(c.var);
            match c.op {
                OP_PLAY => return self.play(&c),
                OP_PLAY_IF_EQUAL if v == c.operand => return self.play(&c),
                OP_PLAY_IF_NOT_EQUAL if v != c.operand => return self.play(&c),
                OP_PLAY_IF_AT_LEAST if v >= c.operand => return self.play(&c),
                OP_PLAY_IF_AT_MOST if v <= c.operand => return self.play(&c),
                OP_END => return None,
                OP_REPEAT => {
                    let mut jump = true;
                    if self.repeat_count > 0 {
                        self.repeat_count -= 1;
                        if self.repeat_count <= 1 {
                            self.repeat_count = 0;
                            jump = false;
                        }
                    } else if self.repeat_count == 0 {
                        self.repeat_count = if c.count > 0 { c.count as i16 } else { -1 };
                    }
                    if jump {
                        self.pc = c.target;
                        continue;
                    }
                }
                OP_SET => self.set(c.var, c.operand),
                OP_ADD => self.set(c.var, v.wrapping_add(c.operand)),
                OP_RANDOM => {
                    let (low, high) = c.range();
                    let range = high as i32 - low as i32;
                    let value = if range <= 0 { low } else { (self.rand() % (range + 1) + low as i32) as i8 };
                    self.set(c.var, value);
                }
                OP_JUMP => {
                    self.pc = c.target;
                    continue;
                }
                OP_JUMP_IF_EQUAL | OP_JUMP_IF_NOT_EQUAL | OP_JUMP_IF_AT_LEAST | OP_JUMP_IF_AT_MOST => {
                    let take = match c.op {
                        OP_JUMP_IF_EQUAL => v == c.operand,
                        OP_JUMP_IF_NOT_EQUAL => v != c.operand,
                        OP_JUMP_IF_AT_LEAST => v >= c.operand,
                        _ => v <= c.operand,
                    };
                    if take {
                        self.pc = c.target;
                        continue;
                    }
                }
                _ => {}
            }
            self.pc += 1;
        }
        None
    }

    /// The samples of the next piece (Read's OpenPiece(Next())); None at
    /// the end of the music. A piece that cannot be played ends it too.
    pub fn next_piece(&mut self) -> Option<Arc<Vec<f32>>> {
        let t = self.next_target()?;
        self.data.piece(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(commands: Vec<Command>) -> Arc<SeqData> {
        let pieces = vec![(0, Some(Arc::new(vec![0.0; 2]))), (2, Some(Arc::new(vec![1.0; 2])))];
        Arc::new(SeqData { commands, pieces, rate: 44100 })
    }

    fn cmd(op: u8, count: u8, var: u8, operand: i8, target: u16) -> Command {
        Command { op, count, var, operand, target }
    }

    #[test]
    fn plays_loops_and_branches() {
        // intro twice, then the loop piece while flag 0 is 0, then end.
        let d = data(vec![
            cmd(OP_PLAY, 2, 0, 0, 0),
            cmd(OP_JUMP_IF_NOT_EQUAL, 0, 0x10, 0, 4),
            cmd(OP_PLAY, 1, 0, 0, 2),
            cmd(OP_JUMP, 0, 0, 0, 1),
            cmd(OP_END, 0, 0, 0, 0),
        ]);
        let flags: SeqFlags = Arc::new(Default::default());
        let mut p = SeqPlayer::new(d, flags.clone());
        let seq: Vec<Option<u16>> = (0..4).map(|_| p.next_target()).collect();
        assert_eq!(seq, vec![Some(0), Some(0), Some(2), Some(2)]);
        flags[0].store(1, Ordering::Relaxed);
        assert_eq!(p.next_target(), None);
    }

    #[test]
    fn repeat_counts() {
        let d = data(vec![cmd(OP_PLAY, 1, 0, 0, 0), cmd(OP_REPEAT, 3, 0, 0, 0), cmd(OP_PLAY, 1, 0, 0, 2), cmd(OP_END, 0, 0, 0, 0)]);
        let mut p = SeqPlayer::new(d, Arc::new(Default::default()));
        let seq: Vec<Option<u16>> = (0..5).map(|_| p.next_target()).collect();
        assert_eq!(seq, vec![Some(0), Some(0), Some(0), Some(2), None]);
    }

    #[test]
    fn mixer_chains_pieces_without_a_gap() {
        let pieces = vec![(0, Some(Arc::new(vec![0.25; 2 * 10]))), (2, Some(Arc::new(vec![0.5; 2 * 10])))];
        let d = Arc::new(SeqData { commands: vec![cmd(OP_PLAY, 1, 0, 0, 0), cmd(OP_PLAY, 1, 0, 0, 2), cmd(OP_END, 0, 0, 0, 0)], pieces, rate: super::super::OUTPUT_RATE });
        let system = super::super::SoundSystem::new(2);
        system.play_music(1, &super::super::Music::Seq(d), false, 1.0);
        let mut out = vec![0.0; 2 * 30];
        system.mixer().mix(&mut out);
        assert!((out[0] - 0.25).abs() < 1e-4);
        assert!((out[2 * 12] - 0.5).abs() < 1e-4, "the second piece follows");
        assert_eq!(out[2 * 25], 0.0, "and the music ends");
        assert!(!system.is_playing(1));
    }
}
