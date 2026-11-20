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
