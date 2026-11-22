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
        #[cfg(target_arch = "wasm32")]
        crate::platform::web::log(level == Level::Error, &format!("[{}] {msg}", format!("{level:?}").to_ascii_lowercase()));
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        eprintln!("[{}] {msg}", format!("{level:?}").to_ascii_lowercase());
    }
}

/// Breadcrumbs kept for the reports: the game thread's last steps (its own
/// list, so another thread's breadcrumb, such as the activity's refresh
/// rate on the Java UI thread, cannot hide where the game thread stopped),
/// another thread's last one, an optional log file and an optional status
/// file (the Android activity shows it on screen).
struct Crumbs {
    start: web_time::Instant,
    game: Option<std::thread::ThreadId>,
    /// The game thread's last steps: (seconds since start, step).
    recent: std::collections::VecDeque<(f32, String)>,
    other: Option<(f32, String, String)>,
    file: Option<std::fs::File>,
    status: Option<(std::path::PathBuf, String)>,
}

/// Game-thread steps kept.
const RECENT: usize = 8;

static CRUMBS: std::sync::Mutex<Option<Crumbs>> = std::sync::Mutex::new(None);

fn with_crumbs<R>(f: impl FnOnce(&mut Crumbs) -> R) -> Option<R> {
    let mut guard = CRUMBS.lock().ok()?;
    let c = guard.get_or_insert_with(|| Crumbs {
        start: web_time::Instant::now(),
        game: None,
        recent: std::collections::VecDeque::new(),
        other: None,
        file: None,
        status: None,
    });
    Some(f(c))
}

/// The calling thread is the game thread (its breadcrumbs are the steps).
pub fn set_game_thread() {
    with_crumbs(|c| c.game = Some(std::thread::current().id()));
}

/// Also writes every breadcrumb to `path` (truncated; the previous file is
/// kept as `prev`).
pub fn set_log_file(path: &std::path::Path, prev: &std::path::Path) {
    let _ = std::fs::rename(path, prev);
    let file = std::fs::File::create(path).ok();
    with_crumbs(|c| c.file = file);
}

/// Keeps `path` holding `header` and the game thread's steps, rewritten at
/// each step, until [`hide_status`].
pub fn show_status(path: &std::path::Path, header: &str) {
    with_crumbs(|c| {
        c.status = Some((path.to_path_buf(), header.to_string()));
        write_status(c);
    });
}

/// Removes the status file.
pub fn hide_status() {
    with_crumbs(|c| {
        if let Some((path, _)) = c.status.take() {
            let _ = std::fs::remove_file(path);
        }
    });
}

/// The status file is being kept.
pub fn status_shown() -> bool {
    with_crumbs(|c| c.status.is_some()).unwrap_or(false)
}

fn steps_text(c: &Crumbs) -> String {
    let mut s = String::new();
    for (t, msg) in &c.recent {
        s.push_str(&format!("{t:7.2}s  {msg}\n"));
    }
    if let Some((t, thread, msg)) = &c.other {
        s.push_str(&format!("(other thread {thread} at {t:.2}s: {msg})\n"));
    }
    s
}

fn write_status(c: &Crumbs) {
    if let Some((path, header)) = &c.status {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, format!("{header}\n{}", steps_text(c))).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

/// The game thread's last steps (newest last), with the time since start,
/// and another thread's last breadcrumb.
pub fn game_steps() -> String {
    with_crumbs(|c| steps_text(c)).unwrap_or_default()
}

/// The game thread's latest step.
pub fn last_breadcrumb() -> String {
    with_crumbs(|c| c.recent.back().map(|r| r.1.clone()).unwrap_or_default()).unwrap_or_default()
}

fn record(msg: &str) {
    use std::io::Write as _;
    with_crumbs(|c| {
        let t = c.start.elapsed().as_secs_f32();
        let thread = std::thread::current();
        let game = c.game.is_none_or(|g| g == thread.id());
        let name = thread.name().unwrap_or("?").to_string();
        if let Some(file) = &mut c.file {
            let _ = writeln!(file, "{t:8.3} {} {msg}", if game { "game" } else { &name });
        }
        if game {
            if c.recent.len() == RECENT {
                c.recent.pop_front();
            }
            c.recent.push_back((t, msg.to_string()));
            write_status(c);
        } else {
            c.other = Some((t, name, msg.to_string()));
        }
    });
}

/// A lifecycle breadcrumb: always in logcat on Android (tag "wf", info),
/// so a crash log shows the steps that led to it; debug level elsewhere.
pub fn breadcrumb(msg: &str) {
    record(msg);
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
