//! Debug aids: the per-frame state hash for replay desync checks.
//!
//! `--hash-log <file>` installs [`log_hash`] as the frame hook, which writes
//! one line per game frame (`frame hash`). Two runs of the same replay must
//! give the same file; the first differing line is the first frame whose
//! state diverged.

use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::Mutex;

use super::Cx;

/// FNV-1a over everything written into it.
struct Fnv(u64);

impl std::fmt::Write for Fnv {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        for b in s.bytes() {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
        Ok(())
    }
}

/// A hash of the state the simulation carries from frame to frame: rand(),
/// g_game and every live actor (the boss generator only consumes rand()). Floats are hashed through their
/// shortest round-trip text, so the hash is exact.
pub fn state_hash(cx: &Cx) -> u64 {
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    let _ = write!(h, "{} {:?} {}", cx.rng.seed, cx.game, cx.actors.count);
    for (i, a) in cx.actors.a.iter().enumerate() {
        if a.flags != 0 {
            let _ = write!(h, "|{i}:{a:?}");
        }
    }
    h.0
}

static HASH_LOG: Mutex<Option<std::io::BufWriter<std::fs::File>>> = Mutex::new(None);

/// Starts logging the state hash of every game frame to `path`.
pub fn start_hash_log(cx: &mut Cx, path: &std::path::Path) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    *HASH_LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::io::BufWriter::new(file));
    cx.frame_hook = Some(log_hash);
    Ok(())
}

fn log_hash(cx: &mut Cx) {
    let hash = state_hash(cx);
    if let Some(out) = HASH_LOG.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = writeln!(out, "{} {hash:016x}", cx.app.frame_count);
        let _ = out.flush();
    }
}
