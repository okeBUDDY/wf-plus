//! The SDL3 implementation of the platform seam: window, keyboard, gamepads
//! (SDL_Gamepad) and audio output.

use super::{Device, Event, Finger, GamepadState, Key, MouseState, PadButton, RawInput};
use crate::config::WindowMode;
use crate::audio::{Mixer, OUTPUT_RATE};
use sdl3::audio::{AudioCallback, AudioFormat, AudioSpec, AudioStream, AudioStreamWithCallback};
use sdl3::event::{Event as SdlEvent, WindowEvent};
use sdl3::gamepad::{Axis, Button, Gamepad};
use sdl3::keyboard::Scancode;

pub struct Platform {
    _sdl: sdl3::Sdl,
    _video: sdl3::VideoSubsystem,
    gamepad_sys: Option<sdl3::GamepadSubsystem>,
    gamepads: Vec<Gamepad>,
    audio_sys: Option<sdl3::AudioSubsystem>,
    audio: Option<AudioStreamWithCallback<MixerCallback>>,
    events: sdl3::EventPump,
    pub window: sdl3::video::Window,
    fingers: Vec<Finger>,
    last_device: Device,
    mouse_inside: bool,
    wheel: f32,
    /// The window's title as created (the FPS display appends to it).
    pub title: String,
    /// Files dropped on the window since the last [`Platform::take_dropped`].
    dropped: Vec<std::path::PathBuf>,
}

pub struct MixerCallback {
    mixer: Mixer,
    buffer: Vec<f32>,
}

impl AudioCallback<f32> for MixerCallback {
    fn callback(&mut self, stream: &mut AudioStream, requested: i32) {
        let n = (requested.max(0) as usize) & !1;
        self.buffer.resize(n, 0.0);
        self.mixer.mix(&mut self.buffer);
        let _ = stream.put_data_f32(&self.buffer);
    }
}

fn keymap(k: Key) -> Scancode {
    match k {
        Key::Up => Scancode::Up,
        Key::Down => Scancode::Down,
        Key::Left => Scancode::Left,
        Key::Right => Scancode::Right,
        Key::Numpad1 => Scancode::Kp1,
        Key::Numpad2 => Scancode::Kp2,
        Key::Numpad3 => Scancode::Kp3,
        Key::Numpad4 => Scancode::Kp4,
        Key::Numpad6 => Scancode::Kp6,
        Key::Numpad7 => Scancode::Kp7,
        Key::Numpad8 => Scancode::Kp8,
        Key::Numpad9 => Scancode::Kp9,
        Key::Z => Scancode::Z,
        Key::X => Scancode::X,
        Key::S => Scancode::S,
        Key::D => Scancode::D,
        Key::A => Scancode::A,
        Key::W => Scancode::W,
        Key::E => Scancode::E,
        Key::F => Scancode::F,
        Key::Escape => Scancode::Escape,
        Key::F12 => Scancode::F12,
        Key::Return => Scancode::Return,
        Key::Space => Scancode::Space,
        Key::Q => Scancode::Q,
        Key::PageUp => Scancode::PageUp,
        Key::PageDown => Scancode::PageDown,
    }
}

const ALL_KEYS: [Key; Key::COUNT] = [
    Key::Up,
    Key::Down,
    Key::Left,
    Key::Right,
    Key::Numpad1,
    Key::Numpad2,
    Key::Numpad3,
    Key::Numpad4,
    Key::Numpad6,
    Key::Numpad7,
    Key::Numpad8,
    Key::Numpad9,
    Key::Z,
    Key::X,
    Key::S,
    Key::D,
    Key::A,
    Key::W,
    Key::E,
    Key::F,
    Key::Escape,
    Key::F12,
    Key::Return,
    Key::Space,
    Key::Q,
    Key::PageUp,
    Key::PageDown,
];

fn padmap(b: PadButton) -> Button {
    match b {
        PadButton::South => Button::South,
        PadButton::East => Button::East,
        PadButton::West => Button::West,
        PadButton::North => Button::North,
        PadButton::LeftShoulder => Button::LeftShoulder,
        PadButton::RightShoulder => Button::RightShoulder,
        PadButton::Back => Button::Back,
        PadButton::Start => Button::Start,
        PadButton::LeftStick => Button::LeftStick,
        PadButton::RightStick => Button::RightStick,
        PadButton::DpadUp => Button::DPadUp,
        PadButton::DpadDown => Button::DPadDown,
        PadButton::DpadLeft => Button::DPadLeft,
        PadButton::DpadRight => Button::DPadRight,
    }
}

const ALL_PAD: [PadButton; PadButton::COUNT] = [
    PadButton::South,
    PadButton::East,
    PadButton::West,
    PadButton::North,
    PadButton::LeftShoulder,
    PadButton::RightShoulder,
    PadButton::Back,
    PadButton::Start,
    PadButton::LeftStick,
    PadButton::RightStick,
    PadButton::DpadUp,
    PadButton::DpadDown,
    PadButton::DpadLeft,
    PadButton::DpadRight,
];

impl Platform {
    /// Opens a resizable window of `width` x `height`.
    pub fn new(title: &str, width: u32, height: u32) -> Result<Self, String> {
        let sdl = sdl3::init().map_err(|e| e.to_string())?;
        let video = sdl.video().map_err(|e| e.to_string())?;
        let window = video
            .window(title, width, height)
            .resizable()
            .high_pixel_density()
            .build()
            .map_err(|e| e.to_string())?;
        // Gamepads and audio are optional: the game runs without them.
        let gamepad_sys = sdl.gamepad().ok();
        let audio_sys = sdl.audio().ok();
        let events = sdl.event_pump().map_err(|e| e.to_string())?;
        // The panel's top refresh rate, unless the Android activity gave
        // it (it also picks that display mode).
        if super::max_refresh().is_none()
            && let Ok(display) = window.get_display()
        {
            let current = display.get_mode().map(|m| m.refresh_rate).unwrap_or(0.0);
            let modes = display.get_fullscreen_modes().unwrap_or_default();
            super::set_max_refresh(modes.iter().map(|m| m.refresh_rate).fold(current, f32::max));
        }
        note_display_size(&window);
        Ok(Platform {
            title: title.to_string(),
            _sdl: sdl,
            _video: video,
            gamepad_sys,
            gamepads: Vec::new(),
            audio_sys,
            audio: None,
            events,
            window,
            fingers: Vec::new(),
            dropped: Vec::new(),
            last_device: Device::Keyboard,
            mouse_inside: false,
            wheel: 0.0,
        })
    }

    /// Starts audio output pulling from `mixer`. Returns false without an
    /// audio device.
    pub fn start_audio(&mut self, mixer: Mixer) -> bool {
        let Some(audio) = &self.audio_sys else { return false };
        let spec = AudioSpec { freq: Some(OUTPUT_RATE as i32), channels: Some(2), format: Some(AudioFormat::f32_sys()) };
        match audio.open_playback_stream(&spec, MixerCallback { mixer, buffer: Vec::new() }) {
            Ok(stream) => {
                let ok = stream.resume().is_ok();
                self.audio = Some(stream);
                ok
            }
            Err(_) => false,
        }
    }

    /// The window's size in pixels.
    pub fn drawable_size(&self) -> (u32, u32) {
        self.window.size_in_pixels()
    }

    /// Handles pending events; returns them for the game.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        for event in self.events.poll_iter() {
            match event {
                SdlEvent::Quit { .. } => out.push(Event::Quit),
                SdlEvent::DropFile { filename, .. } => self.dropped.push(std::path::PathBuf::from(filename)),
                SdlEvent::Window { win_event, .. } => match win_event {
                    WindowEvent::DisplayChanged(_) => note_display_size(&self.window),
                    WindowEvent::FocusGained => out.push(Event::Focus(true)),
                    WindowEvent::FocusLost => out.push(Event::Focus(false)),
                    WindowEvent::PixelSizeChanged(w, h) | WindowEvent::Resized(w, h) => {
                        let _ = (w, h);
                        let (w, h) = self.window.size_in_pixels();
                        out.push(Event::Resized(w, h));
                    }
                    _ => {}
                },
                SdlEvent::GamepadAdded { which, .. } => {
                    if let Some(sys) = &self.gamepad_sys
                        && !self.gamepads.iter().any(|p| p.id().ok() == Some(which))
                        && let Ok(pad) = sys.open(which) {
                            self.gamepads.push(pad);
                            self.last_device = Device::Gamepad;
                        }
                }
                SdlEvent::GamepadRemoved { which, .. } => {
                    self.gamepads.retain(|p| p.id().ok() != Some(which));
                }
                // The last-used device: whatever the player touched last.
                SdlEvent::KeyDown { repeat: false, .. } => self.last_device = Device::Keyboard,
                SdlEvent::MouseMotion { xrel, yrel, .. } => {
                    self.mouse_inside = true;
                    if xrel.abs() + yrel.abs() > 2.0 {
                        self.last_device = Device::Mouse;
                    }
                }
                SdlEvent::MouseButtonDown { .. } => self.last_device = Device::Mouse,
                SdlEvent::MouseWheel { y, .. } => {
                    self.wheel += y;
                    self.last_device = Device::Mouse;
                }
                SdlEvent::GamepadButtonDown { .. } | SdlEvent::GamepadTouchpadDown { .. } => self.last_device = Device::Gamepad,
                SdlEvent::GamepadAxisMotion { value, .. } if value.unsigned_abs() > 16000 => self.last_device = Device::Gamepad,
                SdlEvent::FingerDown { finger_id, x, y, .. } => {
                    self.last_device = Device::Touch;
                    self.fingers.retain(|f| f.id != finger_id);
                    self.fingers.push(Finger { id: finger_id, pos: [x, y] });
                }
                SdlEvent::FingerMotion { finger_id, x, y, .. } => {
                    if let Some(f) = self.fingers.iter_mut().find(|f| f.id == finger_id) {
                        f.pos = [x, y];
                    }
                }
                SdlEvent::FingerUp { finger_id, .. } | SdlEvent::FingerCanceled { finger_id, .. } => {
                    self.fingers.retain(|f| f.id != finger_id);
                }
                _ => {}
            }
        }
        out
    }

    /// The keys and gamepads held now.
    pub fn input(&self) -> RawInput {
        let state = self.events.keyboard_state();
        let mut raw = RawInput::default();
        for k in ALL_KEYS {
            raw.keys[k as usize] = state.is_scancode_pressed(keymap(k));
        }
        for pad in &self.gamepads {
            let mut g = GamepadState::default();
            for b in ALL_PAD {
                if pad.button(padmap(b)) {
                    g.buttons |= 1 << b as u32;
                }
            }
            g.stick = [pad.axis(Axis::LeftX) as f32 / 32767.0, pad.axis(Axis::LeftY) as f32 / 32767.0];
            g.touchpad = touchpad_finger(pad);
            raw.gamepads.push(g);
        }
        let ms = self.events.mouse_state();
        // SDL gives the mouse in window points; the renderer works in pixels.
        let (pw, _) = self.window.size_in_pixels();
        let (lw, _) = self.window.size();
        let density = if lw > 0 { pw as f32 / lw as f32 } else { 1.0 };
        raw.mouse = MouseState {
            pos: [ms.x() * density, ms.y() * density],
            buttons: (ms.left() as u32) | (ms.middle() as u32) << 1 | (ms.right() as u32) << 2 | (ms.x1() as u32) << 3 | (ms.x2() as u32) << 4,
            inside: self.mouse_inside,
            wheel: self.wheel,
        };
        raw.fingers = self.fingers.clone();
        raw.last_device = self.last_device;
        raw.window = self.window.size_in_pixels();
        raw
    }
}

/// SDL_GetGamepadTouchpadFinger of finger 0 on touchpad 0.
fn touchpad_finger(pad: &Gamepad) -> Option<[f32; 2]> {
    if pad.touchpads_count() == 0 {
        return None;
    }
    let (mut down, mut x, mut y, mut p) = (false, 0.0f32, 0.0f32, 0.0f32);
    // SAFETY: an open gamepad and plain out-parameters on the stack.
    let ok = unsafe { sdl3::sys::gamepad::SDL_GetGamepadTouchpadFinger(pad.raw(), 0, 0, &mut down, &mut x, &mut y, &mut p) };
    (ok && down).then_some([x, y])
}

impl Platform {
    /// Resizes the window (windowed mode), in pixels.
    pub fn set_window_size(&mut self, width: u32, height: u32) {
        let (pw, _) = self.window.size_in_pixels();
        let (lw, _) = self.window.size();
        let density = if lw > 0 { pw as f32 / lw as f32 } else { 1.0 };
        let _ = self.window.set_size((width as f32 / density) as u32, (height as f32 / density) as u32);
    }

    /// The window's icon, from straight RGBA pixels (row by row).
    pub fn set_icon(&mut self, width: u32, height: u32, rgba: &[u8]) {
        let mut pixels = rgba.to_vec();
        if let Ok(surface) =
            sdl3::surface::Surface::from_data(&mut pixels, width, height, width * 4, sdl3::pixels::PixelFormat::RGBA32)
        {
            self.window.set_icon(surface);
        }
    }

    /// The files dropped on the window since the last call.
    pub fn take_dropped(&mut self) -> Vec<std::path::PathBuf> {
        std::mem::take(&mut self.dropped)
    }

    pub fn set_title(&mut self, title: &str) {
        let _ = self.window.set_title(title);
    }

    /// Shows the window windowed, exclusive fullscreen (the desktop's
    /// mode) or borderless over the display.
    pub fn set_window_mode(&mut self, mode: WindowMode) {
        use sdl3::sys::video::{SDL_GetDesktopDisplayMode, SDL_GetDisplayForWindow, SDL_SetWindowFullscreenMode};
        let raw = self.window.raw();
        match mode {
            WindowMode::Windowed => {
                let _ = self.window.set_fullscreen(false);
            }
            WindowMode::Borderless => {
                // SAFETY: a live window; a null mode is borderless desktop.
                unsafe { SDL_SetWindowFullscreenMode(raw, std::ptr::null()) };
                let _ = self.window.set_fullscreen(true);
            }
            WindowMode::Fullscreen => {
                // SAFETY: a live window; the desktop mode SDL returns lives
                // as long as the display.
                unsafe {
                    let mode = SDL_GetDesktopDisplayMode(SDL_GetDisplayForWindow(raw));
                    SDL_SetWindowFullscreenMode(raw, mode);
                }
                let _ = self.window.set_fullscreen(true);
            }
        }
    }
}

/// Records the resolution of the window's display, in pixels (the size a
/// borderless or fullscreen window takes; the options show it).
fn note_display_size(window: &sdl3::video::Window) {
    if let Ok(mode) = window.get_display().and_then(|d| d.get_mode()) {
        let px = |v: i32| (v as f32 * mode.pixel_density.max(1.0)).round() as u32;
        super::set_display_size(px(mode.w), px(mode.h));
    }
}

/// The window's display connection, owned so a GPU instance can keep it.
#[derive(Debug)]
pub struct DisplayHandle(raw_window_handle::RawDisplayHandle);

// SAFETY: the handle is only an address of the display connection, which
// SDL keeps open for as long as the Platform (and so the window) lives; the
// renderer is dropped before the Platform.
unsafe impl Send for DisplayHandle {}
unsafe impl Sync for DisplayHandle {}

impl raw_window_handle::HasDisplayHandle for DisplayHandle {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        // SAFETY: see above.
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(self.0) })
    }
}

impl Platform {
    pub fn display_handle(&self) -> Result<DisplayHandle, String> {
        use raw_window_handle::HasDisplayHandle;
        Ok(DisplayHandle(self.window.display_handle().map_err(|e| e.to_string())?.as_raw()))
    }
}

/// Seconds east of UTC of the local time zone, from SDL's clock.
pub fn local_utc_offset() -> i32 {
    use sdl3::sys::time::{SDL_DateTime, SDL_GetCurrentTime, SDL_TimeToDateTime};
    let mut now = 0;
    let mut dt: SDL_DateTime = unsafe { std::mem::zeroed() };
    // SAFETY: plain out-parameters on the stack.
    let ok = unsafe { SDL_GetCurrentTime(&mut now) && SDL_TimeToDateTime(now, &mut dt, true) };
    if ok { dt.utc_offset } else { 0 }
}

/// Opens `url` in the system's browser.
pub fn open_url(url: &str) -> Result<(), String> {
    sdl3::url::open_url(url).map_err(|e| e.to_string())
}
