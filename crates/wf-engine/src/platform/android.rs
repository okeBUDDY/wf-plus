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
/// Every use of the wgpu surface (create, configure, acquire, present)
/// happens under it, after checking the window pointer is still the one
/// the surface was made for. Never held across SDL's event pump (which
/// blocks while the app is paused).
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

/// Asks for `hz` on the native window (ANativeWindow_setFrameRate, API 30+,
/// looked up at run time since the app runs from API 24), with
/// FRAME_RATE_COMPATIBILITY_FIXED_SOURCE: the game renders at the panel's
/// rate, so the system keeps that mode. Under an [`ActivityLock`], with a
/// live window.
pub fn set_frame_rate(window: usize, hz: f32) {
    type SetFrameRate = unsafe extern "C" fn(*mut std::ffi::c_void, f32, i8) -> i32;
    static FUNC: std::sync::OnceLock<Option<SetFrameRate>> = std::sync::OnceLock::new();
    let func = FUNC.get_or_init(|| {
        use sdl3::sys::loadso::{SDL_LoadFunction, SDL_LoadObject};
        // SAFETY: loading libandroid (always present) and looking a symbol
        // up; the pointer is the NDK's declared signature.
        unsafe {
            let lib = SDL_LoadObject(c"libandroid.so".as_ptr());
            if lib.is_null() {
                return None;
            }
            SDL_LoadFunction(lib, c"ANativeWindow_setFrameRate".as_ptr()).map(|f| std::mem::transmute::<unsafe extern "C" fn(), SetFrameRate>(f))
        }
    });
    const FIXED_SOURCE: i8 = 1;
    match func {
        Some(f) if window != 0 => {
            // SAFETY: a live ANativeWindow (the caller holds the lock).
            let r = unsafe { f(window as *mut std::ffi::c_void, hz, FIXED_SOURCE) };
            crate::log::breadcrumb(&format!("ANativeWindow_setFrameRate({hz}) = {r}"));
        }
        Some(_) => {}
        None => crate::log::breadcrumb("ANativeWindow_setFrameRate unavailable (API < 30)"),
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
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("?");
        log_at(ERROR, &format!("panic on thread '{name}': {info}"));
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
