//! The browser platform (wasm32): what SDL3 gives the other builds, from
//! the page instead. Files are an in-memory store: the page fills it at
//! start (the game data the player picked, and what the game saved before,
//! from IndexedDB) and saves what the game writes (`wfStore`).

use std::collections::BTreeMap;
use std::path::{Component, Path};
use std::sync::Mutex;

use wasm_bindgen::{JsCast, JsValue};

/// A file in the page's store: its bytes and when it was written
/// (milliseconds since the Unix epoch).
struct File {
    data: Vec<u8>,
    modified: f64,
}

/// The files the page has handed the game, and the ones the game wrote,
/// by normalized path ("rec/stage01.rec").
static FILES: Mutex<BTreeMap<String, File>> = Mutex::new(BTreeMap::new());

/// `path` as a store key: relative, '/'-separated, without "." parts.
fn key(path: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
            Component::ParentDir => {
                parts.pop();
            }
            _ => {}
        }
    }
    parts.join("/")
}

/// Calls `globalThis.wfStore[method](...args)` when the page has a store
/// (it keeps the files in IndexedDB); nothing otherwise.
fn store(method: &str, args: &[JsValue]) {
    let Ok(store) = js_sys::Reflect::get(&js_sys::global(), &"wfStore".into()) else { return };
    if !store.is_object() {
        return;
    }
    if let Ok(f) = js_sys::Reflect::get(&store, &method.into())
        && let Some(f) = f.dyn_ref::<js_sys::Function>()
    {
        let _ = f.apply(&store, &args.iter().collect::<js_sys::Array>());
    }
}

/// Makes `data` readable at `path`, written at `modified` (ms since the
/// epoch; now when None). The page's own files: not saved back.
pub fn put_file(path: impl AsRef<Path>, data: Vec<u8>, modified: Option<f64>) {
    let modified = modified.unwrap_or_else(js_sys::Date::now);
    FILES.lock().unwrap().insert(key(path.as_ref()), File { data, modified });
}

/// The game wrote `data` at `path`: kept, and saved by the page.
pub fn write_file(path: &Path, data: Vec<u8>) {
    let k = key(path);
    let modified = js_sys::Date::now();
    store("put", &[JsValue::from_str(&k), js_sys::Uint8Array::from(data.as_slice()).into(), JsValue::from_f64(modified)]);
    FILES.lock().unwrap().insert(k, File { data, modified });
}

/// Removes the file at `path` (and from the page's store); false if there
/// was none.
pub fn remove_file(path: &Path) -> bool {
    let k = key(path);
    let had = FILES.lock().unwrap().remove(&k).is_some();
    if had {
        store("delete", &[JsValue::from_str(&k)]);
    }
    had
}

/// The names of the files and folders directly in `dir`.
pub fn list_dir(dir: &Path) -> Vec<String> {
    let prefix = key(dir);
    let prefix = if prefix.is_empty() { prefix } else { prefix + "/" };
    let mut names: Vec<String> = FILES
        .lock()
        .unwrap()
        .keys()
        .filter_map(|k| k.strip_prefix(&prefix))
        .map(|rest| rest.split('/').next().unwrap_or(rest).to_string())
        .collect();
    names.dedup();
    names
}

/// The page has a file at `path`.
pub fn has_file(path: &Path) -> bool {
    FILES.lock().unwrap().contains_key(&key(path))
}

/// When the file at `path` was written (ms since the epoch).
pub fn modified(path: &Path) -> Option<f64> {
    FILES.lock().unwrap().get(&key(path)).map(|f| f.modified)
}

pub fn read_file(path: &Path) -> std::io::Result<Vec<u8>> {
    FILES
        .lock()
        .unwrap()
        .get(&key(path))
        .map(|f| f.data.clone())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, format!("{} is not in the page's files", path.display())))
}

/// The local time's offset from UTC, in seconds (as SDL_DateTime's).
pub fn local_utc_offset() -> i32 {
    // getTimezoneOffset is UTC minus local, in minutes.
    -(js_sys::Date::new_0().get_timezone_offset() as i32) * 60
}

pub fn open_url(url: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or("no window")?;
    window.open_with_url_and_target(url, "_blank").map(|_| ()).map_err(|e| format!("{e:?}"))
}

/// A log line, to the browser's console.
pub fn log(error: bool, msg: &str) {
    if error {
        web_sys::console::error_1(&msg.into());
    } else {
        web_sys::console::log_1(&msg.into());
    }
}

// ---------------------------------------------------------------------------
// Audio: the mixer's output scheduled as short WebAudio buffers.

/// Frames mixed per buffer (23 ms at 44.1 kHz).
const AUDIO_CHUNK: usize = 1024;
/// How far ahead of the audio clock buffers are scheduled, in seconds.
const AUDIO_AHEAD: f64 = 0.08;

/// The audio output: [`Audio::pump`] each display frame mixes and
/// schedules what the next [`AUDIO_AHEAD`] seconds need. Browsers start an
/// audio context suspended until the player clicks or presses a key on the
/// page; [`Audio::resume`] is called from those events.
pub struct Audio {
    ctx: web_sys::AudioContext,
    mixer: crate::audio::Mixer,
    /// The audio clock time the next buffer starts at.
    next: f64,
    buf: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Audio {
    pub fn new(mixer: crate::audio::Mixer) -> Option<Audio> {
        use crate::audio::OUTPUT_RATE;
        let opts = web_sys::AudioContextOptions::new();
        opts.set_sample_rate(OUTPUT_RATE as f32);
        // Some browsers refuse a set rate: their own then (the buffers say
        // theirs, so the browser resamples).
        let ctx = web_sys::AudioContext::new_with_context_options(&opts).or_else(|_| web_sys::AudioContext::new()).ok()?;
        Some(Audio { ctx, mixer, next: 0.0, buf: vec![0.0; AUDIO_CHUNK * 2], left: vec![0.0; AUDIO_CHUNK], right: vec![0.0; AUDIO_CHUNK] })
    }

    /// The page got a click or a key: lets the audio start.
    pub fn resume(&self) {
        if self.ctx.state() != web_sys::AudioContextState::Running {
            let _ = self.ctx.resume();
        }
    }

    /// Stops the output (the page is hidden) or restarts it.
    pub fn set_paused(&mut self, paused: bool) {
        let _ = if paused { self.ctx.suspend() } else { self.ctx.resume() };
    }

    /// Mixes and schedules the buffers due.
    pub fn pump(&mut self) {
        if self.ctx.state() != web_sys::AudioContextState::Running {
            return;
        }
        let now = self.ctx.current_time();
        // Behind the clock (the page was busy or hidden): start again
        // just ahead of it.
        if self.next < now {
            self.next = now + 0.01;
        }
        let rate = crate::audio::OUTPUT_RATE as f32;
        while self.next < now + AUDIO_AHEAD {
            self.mixer.mix(&mut self.buf);
            for i in 0..AUDIO_CHUNK {
                self.left[i] = self.buf[2 * i];
                self.right[i] = self.buf[2 * i + 1];
            }
            let Ok(buffer) = self.ctx.create_buffer(2, AUDIO_CHUNK as u32, rate) else { return };
            let _ = buffer.copy_to_channel(&self.left, 0);
            let _ = buffer.copy_to_channel(&self.right, 1);
            let Ok(source) = self.ctx.create_buffer_source() else { return };
            source.set_buffer(Some(&buffer));
            let _ = source.connect_with_audio_node(&self.ctx.destination());
            let _ = source.start_with_when(self.next);
            self.next += AUDIO_CHUNK as f64 / rate as f64;
        }
    }
}

// ---------------------------------------------------------------------------
// Input: the page's keyboard, pointer (mouse, pen, touch) and gamepads.

use super::{Device, Finger, GamepadState, Key, MouseState, PadButton, RawInput};
use std::cell::RefCell;
use std::rc::Rc;

/// The key a KeyboardEvent.code names, of the ones the game reads.
fn key_of(code: &str) -> Option<Key> {
    Some(match code {
        "ArrowUp" => Key::Up,
        "ArrowDown" => Key::Down,
        "ArrowLeft" => Key::Left,
        "ArrowRight" => Key::Right,
        "Numpad1" => Key::Numpad1,
        "Numpad2" => Key::Numpad2,
        "Numpad3" => Key::Numpad3,
        "Numpad4" => Key::Numpad4,
        "Numpad6" => Key::Numpad6,
        "Numpad7" => Key::Numpad7,
        "Numpad8" => Key::Numpad8,
        "Numpad9" => Key::Numpad9,
        "KeyZ" => Key::Z,
        "KeyX" => Key::X,
        "KeyS" => Key::S,
        "KeyD" => Key::D,
        "KeyA" => Key::A,
        "KeyW" => Key::W,
        "KeyE" => Key::E,
        "KeyF" => Key::F,
        "Escape" => Key::Escape,
        "F12" => Key::F12,
        "Enter" | "NumpadEnter" => Key::Return,
        "Space" => Key::Space,
        "KeyQ" => Key::Q,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        _ => return None,
    })
}

/// The Gamepad API's standard mapping: button index of each PadButton.
const PAD_INDEX: [(PadButton, u32); 14] = [
    (PadButton::South, 0),
    (PadButton::East, 1),
    (PadButton::West, 2),
    (PadButton::North, 3),
    (PadButton::LeftShoulder, 4),
    (PadButton::RightShoulder, 5),
    (PadButton::Back, 8),
    (PadButton::Start, 9),
    (PadButton::LeftStick, 10),
    (PadButton::RightStick, 11),
    (PadButton::DpadUp, 12),
    (PadButton::DpadDown, 13),
    (PadButton::DpadLeft, 14),
    (PadButton::DpadRight, 15),
];

/// What the page's events say. A key, button or finger that went down and
/// up again between two reads is still seen by one read (`pressed`,
/// `mouse_pressed`, `tapped`).
#[derive(Default)]
struct State {
    held: [bool; Key::COUNT],
    pressed: [bool; Key::COUNT],
    mouse: [f32; 2],
    mouse_buttons: u32,
    mouse_pressed: u32,
    inside: bool,
    wheel: f32,
    fingers: Vec<Finger>,
    tapped: Vec<Finger>,
    last_device: Device,
}

/// The page's input, read once per tick with [`Input::read`].
pub struct Input {
    state: Rc<RefCell<State>>,
    canvas: web_sys::HtmlCanvasElement,
}

/// MouseEvent.buttons (1 left, 2 right, 4 middle, 8 back, 16 forward) as
/// MouseState's (left, middle, right, x1, x2).
fn buttons_of(b: u16) -> u32 {
    let b = b as u32;
    (b & 1) | (b & 4) >> 1 | (b & 2) << 1 | (b & 8) | (b & 16)
}

fn listen<E: wasm_bindgen::convert::FromWasmAbi + 'static>(
    target: &web_sys::EventTarget,
    event: &str,
    passive: bool,
    f: impl FnMut(E) + 'static,
) {
    let f = wasm_bindgen::closure::Closure::<dyn FnMut(E)>::new(f);
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_passive(passive);
    let _ = target.add_event_listener_with_callback_and_add_event_listener_options(event, f.as_ref().unchecked_ref(), &opts);
    f.forget();
}

impl Input {
    /// Listens to the page's keys and to the pointer on `canvas`;
    /// `gesture` runs on each click and key (the audio waits for one).
    pub fn attach(canvas: &web_sys::HtmlCanvasElement, gesture: impl Fn() + 'static) -> Input {
        let state = Rc::new(RefCell::new(State::default()));
        let gesture = Rc::new(gesture);
        let window: web_sys::EventTarget = web_sys::window().expect("window").into();
        let target: &web_sys::EventTarget = canvas.as_ref();
        for (event, down) in [("keydown", true), ("keyup", false)] {
            let (s, g) = (state.clone(), gesture.clone());
            listen(&window, event, false, move |e: web_sys::KeyboardEvent| {
                if down {
                    g();
                }
                if let Some(k) = key_of(&e.code()) {
                    let mut s = s.borrow_mut();
                    s.held[k as usize] = down;
                    if down && !e.repeat() {
                        s.pressed[k as usize] = true;
                        s.last_device = Device::Keyboard;
                    }
                    e.prevent_default();
                }
            });
        }
        // Keys held when the page loses focus never see their keyup.
        let s = state.clone();
        listen(&window, "blur", true, move |_: web_sys::Event| {
            let mut s = s.borrow_mut();
            s.held = [false; Key::COUNT];
            s.mouse_buttons = 0;
            s.fingers.clear();
        });
        // Mouse, pen and touch, as pointer events.
        let pos = {
            let canvas = canvas.clone();
            move |e: &web_sys::PointerEvent| -> ([f32; 2], [f32; 2]) {
                let r = canvas.get_bounding_client_rect();
                let (w, h) = (r.width().max(1.0), r.height().max(1.0));
                let (x, y) = (e.client_x() as f64 - r.left(), e.client_y() as f64 - r.top());
                // Pixels of the canvas, and 0..1 of it.
                ([(x * canvas.width() as f64 / w) as f32, (y * canvas.height() as f64 / h) as f32], [(x / w) as f32, (y / h) as f32])
            }
        };
        for event in ["pointerdown", "pointermove", "pointerup", "pointercancel"] {
            let (s, g, pos, canvas) = (state.clone(), gesture.clone(), pos.clone(), canvas.clone());
            listen(target, event, false, move |e: web_sys::PointerEvent| {
                let mut s = s.borrow_mut();
                let (pixels, unit) = pos(&e);
                let id = e.pointer_id() as u64;
                if event == "pointerdown" {
                    g();
                    let _ = canvas.set_pointer_capture(e.pointer_id());
                }
                if e.pointer_type() == "touch" {
                    match event {
                        "pointerdown" => {
                            s.last_device = Device::Touch;
                            s.fingers.retain(|f| f.id != id);
                            s.fingers.push(Finger { id, pos: unit });
                            s.tapped.retain(|f| f.id != id);
                            s.tapped.push(Finger { id, pos: unit });
                        }
                        "pointermove" => {
                            if let Some(f) = s.fingers.iter_mut().find(|f| f.id == id) {
                                f.pos = unit;
                            }
                        }
                        _ => s.fingers.retain(|f| f.id != id),
                    }
                } else {
                    let moved = (pixels[0] - s.mouse[0]).abs() + (pixels[1] - s.mouse[1]).abs();
                    s.mouse = pixels;
                    s.inside = true;
                    let buttons = buttons_of(e.buttons());
                    if event == "pointerdown" || (event == "pointermove" && moved > 2.0) {
                        s.last_device = Device::Mouse;
                    }
                    s.mouse_pressed |= buttons & !s.mouse_buttons;
                    s.mouse_buttons = buttons;
                }
                e.prevent_default();
            });
        }
        let s = state.clone();
        listen(target, "pointerleave", true, move |e: web_sys::PointerEvent| {
            if e.pointer_type() != "touch" {
                s.borrow_mut().inside = false;
            }
        });
        let s = state.clone();
        listen(target, "wheel", false, move |e: web_sys::WheelEvent| {
            // Notches, up positive: pixels (about 100 a notch) or lines (3).
            let per_notch = match e.delta_mode() {
                0 => 100.0,
                1 => 3.0,
                _ => 1.0,
            };
            let mut s = s.borrow_mut();
            s.wheel -= (e.delta_y() / per_notch) as f32;
            s.last_device = Device::Mouse;
            e.prevent_default();
        });
        listen(target, "contextmenu", false, |e: web_sys::Event| e.prevent_default());
        Input { state, canvas: canvas.clone() }
    }

    /// The input now: what is held, plus what was pressed since the last
    /// read.
    pub fn read(&self) -> RawInput {
        let mut s = self.state.borrow_mut();
        let mut raw = RawInput { window: (self.canvas.width(), self.canvas.height()), ..Default::default() };
        for k in 0..Key::COUNT {
            raw.keys[k] = s.held[k] || s.pressed[k];
        }
        s.pressed = [false; Key::COUNT];
        raw.mouse = MouseState { pos: s.mouse, buttons: s.mouse_buttons | s.mouse_pressed, inside: s.inside, wheel: s.wheel };
        s.mouse_pressed = 0;
        raw.fingers = s.fingers.clone();
        for t in std::mem::take(&mut s.tapped) {
            if !raw.fingers.iter().any(|f| f.id == t.id) {
                raw.fingers.push(t);
            }
        }
        if let Some(pads) = web_sys::window().and_then(|w| w.navigator().get_gamepads().ok()) {
            for pad in pads.iter() {
                let Ok(pad) = pad.dyn_into::<web_sys::Gamepad>() else { continue };
                if !pad.connected() {
                    continue;
                }
                let buttons = pad.buttons();
                let mut g = GamepadState::default();
                for (b, i) in PAD_INDEX {
                    if let Ok(button) = buttons.get(i).dyn_into::<web_sys::GamepadButton>()
                        && button.pressed()
                    {
                        g.buttons |= 1 << b as u32;
                    }
                }
                let axes = pad.axes();
                let axis = |i| axes.get(i).as_f64().unwrap_or(0.0) as f32;
                g.stick = [axis(0), axis(1)];
                if g.buttons != 0 || g.stick[0].abs() > 0.5 || g.stick[1].abs() > 0.5 {
                    s.last_device = Device::Gamepad;
                }
                raw.gamepads.push(g);
            }
        }
        raw.last_device = s.last_device;
        raw
    }
}
