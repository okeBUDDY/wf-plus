//! The game as a library: libwfplus.so on Android (see android.rs) and the
//! browser's wasm module (see web.rs). The game is the same code as the
//! desktop binary, built as a module of this library. On other targets
//! this library is empty.

#![cfg(any(target_os = "android", target_arch = "wasm32"))]

#[path = "main.rs"]
mod wf_main;

// The game's modules refer to each other as `crate::...`.
#[allow(unused_imports)]
pub(crate) use wf_main::{app_info, art, data, data_import, game, gfx, log, platform, setup, shape_table, sound};

#[cfg(target_os = "android")]
mod android;
