//! The SDL3 implementation of the platform seam: window, keyboard, gamepads
//! (SDL_Gamepad) and audio output.

use super::{Event, GamepadState, Key, PadButton, RawInput};
use crate::sound::{Mixer, OUTPUT_RATE};
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
        Ok(Platform { _sdl: sdl, _video: video, gamepad_sys, gamepads: Vec::new(), audio_sys, audio: None, events, window })
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
                SdlEvent::Window { win_event, .. } => match win_event {
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
                    if let Some(sys) = &self.gamepad_sys {
                        if let Ok(pad) = sys.open(which) {
                            self.gamepads.push(pad);
                        }
                    }
                }
                SdlEvent::GamepadRemoved { which, .. } => {
                    self.gamepads.retain(|p| p.id().ok() != Some(which));
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
            raw.gamepads.push(g);
        }
        raw
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
