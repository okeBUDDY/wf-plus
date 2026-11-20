//! The Android entry point. WarningForeverActivity loads libwfplus.so (SDL3 is linked
//! into it statically) and calls `SDL_main` on its SDL thread. The game is
//! the same code as the desktop binary, built as a module of this library.
//! On other targets this library is empty.

#![cfg(target_os = "android")]

#[path = "main.rs"]
mod wf_main;

// The game's modules refer to each other as `crate::...`.
#[allow(unused_imports)]
pub(crate) use wf_main::{app_info, art, data, game, gfx, log, platform, shape_table, sound};

use std::ffi::{c_char, c_int};

#[unsafe(no_mangle)]
pub extern "C" fn SDL_main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
    wf_main::android_main()
}

/// WarningForeverActivity.nativeSetMaxRefreshRate: the panel's highest
/// refresh rate at the current resolution (the display mode the activity
/// asks for), for the options' FPS caps and the surface's frame rate.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_okebuddy_wfplus_WarningForeverActivity_nativeSetMaxRefreshRate(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    hz: f32,
) {
    platform::set_max_refresh(hz);
    log::breadcrumb(&format!("panel max refresh {hz} Hz"));
}

/// WarningForeverActivity.nativeSetOverrides: the intent extras `backend`
/// (0 none, 1 vulkan, 2 gl) and `refresh` (0 none, 1 on, 2 off), before
/// SDL_main starts.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_okebuddy_wfplus_WarningForeverActivity_nativeSetOverrides(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    backend: i32,
    refresh: i32,
) {
    platform::android::set_intent_overrides(backend, refresh);
}

/// WarningForeverActivity.nativeRefreshState: 0 not decided yet (ask
/// again), 1 request the panel's top refresh rate, 2 do not (safe mode or
/// refresh = off).
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_okebuddy_wfplus_WarningForeverActivity_nativeRefreshState(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) -> i32 {
    platform::android::refresh_state() as i32
}

/// WarningForeverActivity.nativeSetPaused: onPause (true) and onResume
/// (false), for the watchdog.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_okebuddy_wfplus_WarningForeverActivity_nativeSetPaused(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    paused: u8,
) {
    platform::android::set_activity_paused(paused != 0);
}

/// WarningForeverActivity.nativeDismissStatus: the overlay's close bar was
/// tapped.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_okebuddy_wfplus_WarningForeverActivity_nativeDismissStatus(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) {
    platform::android::dismiss_status();
}
