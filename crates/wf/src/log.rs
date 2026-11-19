//! The logging seam: the game reports problems here, and the frontend
//! decides where they go (stderr by default). The original showed most of
//! these in a MessageBox or not at all.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

static LEVEL: AtomicU8 = AtomicU8::new(Level::Warn as u8);

/// Shows messages up to `level` (WF_LOG=error|warn|info|debug).
pub fn set_level(level: Level) {
    LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn init_from_env() {
    if let Ok(v) = std::env::var("WF_LOG") {
        set_level(match v.to_ascii_lowercase().as_str() {
            "error" => Level::Error,
            "info" => Level::Info,
            "debug" => Level::Debug,
            _ => Level::Warn,
        });
    }
}

pub fn enabled(level: Level) -> bool {
    level as u8 <= LEVEL.load(Ordering::Relaxed)
}

pub fn log(level: Level, msg: &str) {
    if enabled(level) {
        eprintln!("[{}] {msg}", format!("{level:?}").to_ascii_lowercase());
    }
}

pub fn error(msg: &str) {
    log(Level::Error, msg)
}
pub fn warn(msg: &str) {
    log(Level::Warn, msg)
}
pub fn info(msg: &str) {
    log(Level::Info, msg)
}
pub fn debug(msg: &str) {
    log(Level::Debug, msg)
}
