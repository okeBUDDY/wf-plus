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
        #[cfg(target_os = "android")]
        crate::platform::android::log(level == Level::Error, msg);
        #[cfg(not(target_os = "android"))]
        eprintln!("[{}] {msg}", format!("{level:?}").to_ascii_lowercase());
    }
}

static LAST_BREADCRUMB: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// The latest breadcrumb (the startup watchdog's report).
pub fn last_breadcrumb() -> String {
    LAST_BREADCRUMB.lock().map(|s| s.clone()).unwrap_or_default()
}

/// A lifecycle breadcrumb: always in logcat on Android (tag "wf", info),
/// so a crash log shows the steps that led to it; debug level elsewhere.
pub fn breadcrumb(msg: &str) {
    if let Ok(mut last) = LAST_BREADCRUMB.lock() {
        last.clear();
        last.push_str(msg);
    }
    #[cfg(target_os = "android")]
    crate::platform::android::log_at(crate::platform::android::INFO, &format!("lifecycle: {msg}"));
    #[cfg(not(target_os = "android"))]
    debug(&format!("lifecycle: {msg}"));
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
