//! Android specifics: where data.bin and the home directory are, file reads
//! that reach into the APK's assets, and logging to logcat.

use std::ffi::{CString, c_char, c_int};
use std::path::PathBuf;

unsafe extern "C" {
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    // SDL's activity mutex (src/core/android/SDL_android.c, SDL 3.4.20; SDL
    // is linked statically, so its internal symbols resolve). The Java UI
    // thread holds it in onNativeSurfaceCreated/Changed/Destroyed while it
    // sets or releases the ANativeWindow.
    fn Android_LockActivityMutex();
    fn Android_UnlockActivityMutex();
}

/// logcat priorities.
pub const INFO: c_int = 4;
pub const WARN: c_int = 5;
pub const ERROR: c_int = 6;

/// Writes one line to logcat under the tag "wf".
pub fn log_at(prio: c_int, msg: &str) {
    let text = CString::new(msg.replace('\0', " ")).unwrap_or_default();
    // SAFETY: both strings are NUL terminated and outlive the call.
    unsafe { __android_log_write(prio, c"wf".as_ptr(), text.as_ptr()) };
}

/// Writes one line to logcat under the tag "wf", as an error or a warning.
pub fn log(error: bool, msg: &str) {
    log_at(if error { ERROR } else { WARN }, msg);
}

/// Holds SDL's activity mutex: while it is held, the Java thread cannot
/// release the window's ANativeWindow (surfaceDestroyed) or set a new one.
/// Held only to read the window pointer and make a wgpu surface on it (see
/// app::Display); never across a GPU call that can wait, nor SDL's event
/// pump (which blocks while the app is paused).
pub struct ActivityLock(());

impl ActivityLock {
    #[allow(clippy::new_without_default)] // a lock taken, not a default value
    pub fn new() -> Self {
        // SAFETY: SDL_LockMutex on SDL's (recursive) mutex; a null mutex
        // (SDL not set up) is a no-op.
        unsafe { Android_LockActivityMutex() };
        ActivityLock(())
    }
}

impl Drop for ActivityLock {
    fn drop(&mut self) {
        // SAFETY: paired with the lock in `new`, on the same thread.
        unsafe { Android_UnlockActivityMutex() };
    }
}

/// The window's ANativeWindow as SDL holds it now (null once
/// surfaceDestroyed released it). Read under an [`ActivityLock`].
pub fn native_window(window: &sdl3::video::Window) -> usize {
    use sdl3::sys::properties::SDL_GetPointerProperty;
    use sdl3::sys::video::{SDL_GetWindowProperties, SDL_PROP_WINDOW_ANDROID_WINDOW_POINTER};
    // SAFETY: the window is alive; SDL returns the property or null.
    unsafe {
        let props = SDL_GetWindowProperties(window.raw());
        SDL_GetPointerProperty(props, SDL_PROP_WINDOW_ANDROID_WINDOW_POINTER, std::ptr::null_mut()) as usize
    }
}

/// Logs panics to logcat (with where they happened) before the default
/// hook runs: a panic on any thread then leaves a readable crash log.
/// The first panic's message, for the message box shown when SDL_main
/// catches it.
static PANIC: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn panic_message() -> Option<String> {
    PANIC.lock().ok().and_then(|p| p.clone())
}

pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("?");
        let msg = format!("panic on thread '{name}': {info}");
        log_at(ERROR, &msg);
        if let Ok(mut last) = PANIC.lock() {
            last.get_or_insert(msg);
        }
        log_at(ERROR, &format!("backtrace:\n{}", std::backtrace::Backtrace::force_capture()));
        default(info);
    }));
}

/// The app-specific external storage (/sdcard/Android/data/<package>/files),
/// which needs no permission and which the user can reach over USB or adb;
/// else the internal storage.
fn storage() -> PathBuf {
    use sdl3::sys::system::{SDL_GetAndroidExternalStoragePath, SDL_GetAndroidInternalStoragePath};
    // SAFETY: SDL returns a NUL-terminated string it owns, or null.
    let get = |p: *const c_char| (!p.is_null()).then(|| PathBuf::from(unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned()));
    get(unsafe { SDL_GetAndroidExternalStoragePath() })
        .or_else(|| get(unsafe { SDL_GetAndroidInternalStoragePath() }))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// (data.bin, home). A data.bin the user put in the app's storage overrides
/// the one bundled in the APK, which is read from the assets by the
/// relative path "data.bin" (SDL_IOFromFile looks in the APK's assets).
pub fn paths() -> (PathBuf, PathBuf) {
    let home = storage();
    let own = home.join("data.bin");
    let data = if own.is_file() { own } else { PathBuf::from("data.bin") };
    (data, home)
}

/// Reads a whole file: a filesystem path, or a relative path into the APK's
/// assets, through SDL's IO.
pub fn read_file(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut io = sdl3::iostream::IOStream::from_file(path, "rb").map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e.to_string()))?;
    let mut out = Vec::new();
    io.read_to_end(&mut out)?;
    Ok(out)
}

/// Shows a fatal error, as the original's Shutdown() MessageBox.
pub fn fatal(msg: &str) {
    log(true, msg);
    let _ = sdl3::messagebox::show_simple_message_box(sdl3::messagebox::MessageBoxFlag::ERROR, "WARNING FOREVER", msg, None);
}

// Launch modes, safe mode and the on-screen steps.
//
// A launch that never shows a frame leaves `wf-launch-pending` in the
// storage dir (its mode inside). The next launch then runs in safe mode:
// the GL backend instead of Vulkan, no refresh-rate requests, and the game
// thread's steps on screen (the activity shows `wf-status.txt`). A safe
// launch that also fails is followed by a normal one, so a launch killed
// before its first frame for another reason never locks the app in safe
// mode. wf-plus.ini (`[android] backend = gl|vulkan`, `refresh = on|off`) and
// the intent extras `backend` and `refresh` (adb: `am start -n
// io.github.okebuddy.wfplus/.WarningForeverActivity --es backend gl --es
// refresh off`) override both modes; the extras win.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// From the intent: 0 none, 1 Vulkan, 2 GL.
static INTENT_BACKEND: AtomicU8 = AtomicU8::new(0);
/// From the intent: 0 none, 1 on, 2 off.
static INTENT_REFRESH: AtomicU8 = AtomicU8::new(0);
/// 0 undecided (SDL_main has not started), 1 allowed, 2 off.
static REFRESH: AtomicU8 = AtomicU8::new(0);
/// 0 auto, 1 Vulkan, 2 GL.
static BACKEND: AtomicU8 = AtomicU8::new(0);
static SAFE: AtomicBool = AtomicBool::new(false);
/// wf-plus.ini `android.status_overlay = off`: never show the steps.
static OVERLAY_OFF: AtomicBool = AtomicBool::new(false);
/// The activity is paused (onPause .. onResume), from Java.
static PAUSED: AtomicBool = AtomicBool::new(false);
/// When the activity last resumed.
static RESUMED_AT: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// The activity's onPause (true) and onResume (false). The game thread
/// cannot report this itself: with SDL_ANDROID_BLOCK_ON_PAUSE it is
/// blocked in the event pump until the resume.
pub fn set_activity_paused(paused: bool) {
    PAUSED.store(paused, Ordering::Relaxed);
    if !paused && let Ok(mut t) = RESUMED_AT.lock() {
        *t = Some(std::time::Instant::now());
    }
}

/// Paused, or resumed less than `grace` ago (the watchdog waits then).
pub fn paused_or_resuming(grace: std::time::Duration) -> bool {
    PAUSED.load(Ordering::Relaxed) || RESUMED_AT.lock().ok().and_then(|t| *t).is_some_and(|t| t.elapsed() < grace)
}

/// Shows `header` and the game thread's steps on screen (unless
/// android.status_overlay = off); taken away by [`status_tick`] once
/// frames have flowed for 10 s, or by a tap on it.
pub fn show_status(header: &str) {
    if !OVERLAY_OFF.load(Ordering::Relaxed) {
        crate::log::show_status(&status_path(), header);
    }
}

/// The activity's close bar was tapped.
pub fn dismiss_status() {
    crate::log::breadcrumb("status overlay closed (tap)");
    crate::log::hide_status();
}

/// The activity's intent extras (codes as in the statics above).
pub fn set_intent_overrides(backend: i32, refresh: i32) {
    INTENT_BACKEND.store(backend.clamp(0, 2) as u8, Ordering::Relaxed);
    INTENT_REFRESH.store(refresh.clamp(0, 2) as u8, Ordering::Relaxed);
}

/// For the activity: 0 ask again later, 1 request the top refresh rate,
/// 2 do not.
pub fn refresh_state() -> u8 {
    REFRESH.load(Ordering::Relaxed)
}

/// The backends to use; None for wgpu's default.
pub fn backends() -> Option<wgpu::Backends> {
    match BACKEND.load(Ordering::Relaxed) {
        1 => Some(wgpu::Backends::VULKAN),
        2 => Some(wgpu::Backends::GL),
        _ => None,
    }
}

pub fn safe_mode() -> bool {
    SAFE.load(Ordering::Relaxed)
}

/// The file the activity shows on screen while it exists.
pub fn status_path() -> PathBuf {
    storage().join("wf-status.txt")
}

/// Decides this launch's mode (call first thing in SDL_main): starts the
/// log file (wf-log.txt, the previous launch's kept as wf-log-prev.txt),
/// reads wf-plus.ini and the intent, and leaves the pending marker.
pub fn launch() {
    let dir = storage();
    crate::log::set_log_file(&dir.join("wf-log.txt"), &dir.join("wf-log-prev.txt"));
    let pending = dir.join("wf-launch-pending");
    let prev = std::fs::read_to_string(&pending).ok();
    let safe = prev.as_deref().map(str::trim) == Some("normal");
    if let Err(e) = std::fs::write(&pending, if safe { "safe" } else { "normal" }) {
        crate::log::breadcrumb(&format!("cannot write {}: {e}", pending.display()));
    }
    let ini = crate::config::Config::load(&dir.join("wf-plus.ini"));
    let backend = match INTENT_BACKEND.load(Ordering::Relaxed) {
        0 => match ini.get("android.backend").map(str::to_ascii_lowercase).as_deref() {
            Some("vulkan") => 1,
            Some("gl" | "gles" | "opengl") => 2,
            _ if safe => 2,
            _ => 0,
        },
        b => b,
    };
    let refresh = match INTENT_REFRESH.load(Ordering::Relaxed) {
        0 => match ini.get("android.refresh").map(str::to_ascii_lowercase).as_deref() {
            Some("off" | "false" | "0") => 2,
            Some("on" | "true" | "1") => 1,
            _ if safe => 2,
            _ => 1,
        },
        r => r,
    };
    let overlay_off = matches!(ini.get("android.status_overlay").map(str::to_ascii_lowercase).as_deref(), Some("off" | "false" | "0"));
    OVERLAY_OFF.store(overlay_off, Ordering::Relaxed);
    BACKEND.store(backend, Ordering::Relaxed);
    REFRESH.store(refresh, Ordering::Relaxed);
    SAFE.store(safe, Ordering::Relaxed);
    let backend_name = ["auto", "Vulkan", "GL"][backend as usize];
    let refresh_name = if refresh == 2 { "off" } else { "on" };
    crate::log::breadcrumb(&format!(
        "launch: {} mode (last launch: {}), backend {backend_name}, refresh requests {refresh_name}; log {}",
        if safe { "SAFE" } else { "normal" },
        match prev.as_deref().map(str::trim) {
            None => "showed frames",
            Some("normal") => "normal, no frame",
            Some(_) => "safe, no frame",
        },
        dir.join("wf-log.txt").display()
    ));
    // A status file from an earlier process (one killed while showing it)
    // must not outlive it: hide_status only knows this process's.
    crate::log::hide_status();
    let _ = std::fs::remove_file(status_path());
    if safe {
        show_status(
            &format!(
                "WARNING FOREVER+ SAFE MODE: the last start showed no frame.\nRenderer backend {backend_name}, refresh-rate requests {refresh_name}.\nThe next start is normal again. Steps (also in {}):",
                dir.join("wf-log.txt").display()
            ),
        );
    }
}

/// The first frame is on screen: the launch worked.
pub fn first_frame() {
    let _ = std::fs::remove_file(storage().join("wf-launch-pending"));
}

/// Called every half second by the watchdog with the time frames have
/// been flowing without a break: takes the on-screen steps away once that
/// reaches 10 s (every time they are shown, not just once).
pub fn status_tick(flowing: Option<std::time::Duration>) {
    if flowing.is_some_and(|d| d >= std::time::Duration::from_secs(10)) {
        crate::log::hide_status();
    }
}
