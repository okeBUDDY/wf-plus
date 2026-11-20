//! Android specifics: where data.bin and the home directory are, file reads
//! that reach into the APK's assets, and logging to logcat.

use std::ffi::{CString, c_char, c_int};
use std::path::PathBuf;

unsafe extern "C" {
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

/// Writes one line to logcat under the tag "wf".
pub fn log(error: bool, msg: &str) {
    let text = CString::new(msg.replace('\0', " ")).unwrap_or_default();
    // ANDROID_LOG_ERROR = 6, ANDROID_LOG_WARN = 5.
    // SAFETY: both strings are NUL terminated and outlive the call.
    unsafe { __android_log_write(if error { 6 } else { 5 }, c"wf".as_ptr(), text.as_ptr()) };
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
