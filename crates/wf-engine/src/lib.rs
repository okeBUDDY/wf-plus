//! The engine layer under the Warning Forever port.
//!
//! * [`platform`]: the SDL3 window, events, devices and audio output, behind
//!   plain types (the only module that sees SDL).
//! * [`gfx`]: the render snapshot ([`gfx::DrawList`]) and the wgpu backend
//!   that draws it with Slug from the cooked art.
//! * [`audio`]: the software mixer and the decoders.
//! * [`log`]: the logging seam.
//!
//! The game sits on top as deterministic state: it reads abstract input,
//! emits a render snapshot and audio commands, and never touches the
//! platform or the GPU.

#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod app;
pub mod assets;
pub mod audio;
pub mod config;
pub mod gfx;
pub mod input;
pub mod log;
pub mod platform;
pub mod time;
