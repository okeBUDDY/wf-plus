//! The browser entry points (wasm-bindgen). The page hands the game its
//! files (`put_file`: the player's data.bin, replays), then either plays a
//! replay without drawing (`run_replay`, the determinism check) or starts
//! the game on a canvas (`start`): wgpu on WebGL2, ticked from
//! requestAnimationFrame at the game's fixed rate, with the page's input
//! and WebAudio (see `platform::web`). What the game saves goes to the
//! page's store (IndexedDB, see web/index.html).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use super::{Options, WfGame, art, gfx, platform, setup, startup};
use platform::RawInput;
use wf_engine::app::{Game, Snapshots};
use wf_engine::time::{FixedStep, TICK};

/// Where the page puts the game data in the file store: data.bin, or the
/// download (wf108.zip, read in place).
const DATA: &str = "data.bin";
const ZIP: &str = "wf108.zip";

fn options(replay: Option<String>) -> Options {
    let mut o = Options::new();
    o.data = Some(if platform::web::has_file(ZIP.as_ref()) {
        setup::Found::Zip(PathBuf::from(ZIP))
    } else {
        setup::Found::File(PathBuf::from(DATA))
    });
    o.home = PathBuf::new();
    o.replay = replay;
    o
}

fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

/// WF_LOG's levels: error, warn, info, debug.
#[wasm_bindgen]
pub fn set_log_level(level: &str) {
    use wf_engine::log::Level;
    wf_engine::log::set_level(match level {
        "error" => Level::Error,
        "info" => Level::Info,
        "debug" => Level::Debug,
        _ => Level::Warn,
    });
}

/// Makes `data` the file at `path` (relative: "data.bin" or "wf108.zip",
/// "rec/stage01.rec"), written at `modified` (ms since the epoch; now if
/// absent). The page's files (the game data, what IndexedDB kept).
#[wasm_bindgen]
pub fn put_file(path: &str, data: Vec<u8>, modified: Option<f64>) {
    platform::web::put_file(path, data, modified);
}

/// The headless game the native `--headless` builds, at its size.
fn headless(o: &Options) -> Result<(WfGame, wf_engine::input::InputSystem), JsError> {
    let (cx, app) = startup(o, art::Art::load()).map_err(js_err)?;
    let mut game = WfGame { cx, app, synced: None, touching: false };
    let size = (1280, 720);
    game.cx.window_arena_width = wf_engine::app::window_arena(size)[0];
    game.cx.window_size = size;
    let input = wf_engine::input::InputSystem::new(wf_engine::input::Bindings::from_config(&game.cx.port));
    Ok((game, input))
}

/// Plays the replay at `path` for up to `frames` ticks without drawing, as
/// `wfplus --headless --replay PATH --frames N` does; the stage's result
/// ("cleared ... score N" or "game over") is logged at the info level.
/// Returns the ticks run.
#[wasm_bindgen]
pub fn run_replay(path: &str, frames: u32) -> Result<u32, JsError> {
    let o = options(Some(path.into()));
    let (mut game, mut input) = headless(&o)?;
    wf_engine::app::run_ticks(&mut game, &mut input, frames, |_| RawInput::default());
    Ok(game.cx.app.frame_count)
}

/// The running game on a canvas.
struct Web {
    game: WfGame,
    input: wf_engine::input::InputSystem,
    page: platform::web::Input,
    audio: Option<Rc<RefCell<platform::web::Audio>>>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: gfx::gpu::Renderer,
    scaler: gfx::scale::Scaler,
    step: FixedStep,
    snaps: Snapshots,
    canvas: web_sys::HtmlCanvasElement,
    /// Render target pixels per canvas pixel (the options' RENDER SCALE).
    scaler_scale: f32,
    /// The page was hidden at the last frame.
    hidden: bool,
    fps: wf_engine::time::FpsCounter,
    /// When the last frame was shown, and the smoothed time between frames.
    presented: Option<web_time::Instant>,
    frame_ms: f32,
}

impl Web {
    /// One display frame: the ticks due, then the blended snapshot.
    fn frame(&mut self) {
        let hidden = web_sys::window().and_then(|w| w.document()).is_some_and(|d| d.hidden());
        if hidden != self.hidden {
            // A hidden page gets no frames: the game waits as an app in
            // the background does, and does not catch up when it is back.
            self.hidden = hidden;
            self.game.background(hidden);
            if let Some(a) = &self.audio {
                a.borrow_mut().set_paused(hidden);
            }
            self.step.reset();
        }
        let size = (self.canvas.width().max(1), self.canvas.height().max(1));
        if size != (self.config.width, self.config.height) {
            (self.config.width, self.config.height) = size;
            self.surface.configure(self.renderer.device(), &self.config);
        }
        let game = &mut self.game;
        game.set_window_arena(wf_engine::app::window_arena(size));
        game.set_window_size(size);
        if let Some(d) = game.display_request() {
            self.scaler_scale = d.render_scale;
        }
        for _ in 0..self.step.advance(web_time::Instant::now()) {
            game.sync_bindings(&mut self.input.bindings);
            let layout = gfx::Layout::new(self.snaps.cur.arena, size);
            let actions = if game.active() { self.input.update(&self.page.read(), &layout) } else { Default::default() };
            let made = game.tick(&actions);
            self.snaps.push(made, game);
            if !game.running() {
                return;
            }
        }
        if let Some(a) = &self.audio {
            a.borrow_mut().pump();
        }
        self.renderer.time = game.shader_time();
        let layout = gfx::Layout::new(self.snaps.cur.arena, size);
        let border = game.wants_border();
        let list = self.snaps.frame(self.step.alpha(), &layout, border);
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => {
                // Lost or outdated: configured again next frame.
                self.config.width = 0;
                return;
            }
        };
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.scaler.render(&mut self.renderer, list, &view, size, self.scaler_scale);
        self.renderer.queue().present(texture);
        // SHOW FPS: frames shown per second, and the frame time (present to
        // present, smoothed; WebGL has no GPU timestamps).
        let now = web_time::Instant::now();
        self.fps.frame(now);
        if let Some(at) = self.presented {
            let ms = now.duration_since(at).as_secs_f32() * 1e3;
            self.frame_ms = if self.frame_ms == 0.0 { ms } else { self.frame_ms + (ms - self.frame_ms) * 0.05 };
        }
        self.presented = Some(now);
        self.game.set_fps(self.fps.fps as f32);
        self.game.set_frame_time(self.renderer.gpu_ms(), self.frame_ms);
    }
}

/// A panic goes to the console and to the page's error screen
/// (`globalThis.wfFatal`), not only to stderr, which a browser does not
/// have.
fn set_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("The game stopped with an error: {info}");
        wf_engine::log::error(&msg);
        if let Ok(f) = js_sys::Reflect::get(&js_sys::global(), &"wfFatal".into())
            && let Some(f) = f.dyn_ref::<js_sys::Function>()
        {
            let _ = f.call1(&JsValue::NULL, &msg.into());
        }
    }));
}

/// Starts the game on `canvas` (the title, or the replay at `replay`), on
/// WebGL2, with the page's keyboard, pointer and gamepads and WebAudio.
/// Resolves once the first frame is scheduled; when the player quits (the
/// title's EXIT), `on_quit` is called.
#[wasm_bindgen]
pub async fn start(canvas: web_sys::HtmlCanvasElement, replay: Option<String>, on_quit: Option<js_sys::Function>) -> Result<(), JsError> {
    set_panic_hook();
    let o = options(replay);
    let (cx, app) = startup(&o, art::Art::load()).map_err(js_err)?;
    let game = WfGame { cx, app, synced: None, touching: false };
    let input = wf_engine::input::InputSystem::new(wf_engine::input::Bindings::from_config(&game.cx.port));

    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    desc.backends = wgpu::Backends::GL;
    let instance = wgpu::Instance::new(desc);
    let surface = instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone())).map_err(js_err)?;
    let (adapter, device, queue) = gfx::gpu::request_device(&instance, Some(&surface)).await.map_err(js_err)?;
    let info = adapter.get_info();
    wf_engine::log::info(&format!("GPU: {} ({:?})", info.name, info.backend));
    let caps = surface.get_capabilities(&adapter);
    // Blend in gamma space, as the original's Direct3D 7 did.
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .or_else(|| caps.formats.first().copied())
        .ok_or_else(|| js_err("the canvas has no surface format"))?;
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: canvas.width().max(1),
        height: canvas.height().max(1),
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
        color_space: Default::default(),
    };
    surface.configure(&device, &config);
    let mut renderer = gfx::gpu::Renderer::new(device, queue, format, &game.cx.art.pack);
    renderer.set_sprite_cache(game.cx.settings.sprite_cache);
    renderer.analytic_shapes = game.cx.settings.analytic_shapes;
    game.cx.art.register_line_shapes(&mut renderer);
    let scaler = gfx::scale::Scaler::new(&renderer);

    let audio = platform::web::Audio::new(game.cx.sound.mixer()).map(|a| Rc::new(RefCell::new(a)));
    if audio.is_none() {
        wf_engine::log::warn("no WebAudio: no sound");
    }
    let gesture = {
        let audio = audio.clone();
        move || {
            if let Some(a) = &audio {
                a.borrow().resume();
            }
        }
    };
    gesture();
    let page = platform::web::Input::attach(&canvas, gesture);
    let scaler_scale = game.cx.settings.display.render_scale;

    let web = Rc::new(RefCell::new(Web {
        game,
        input,
        page,
        audio,
        surface,
        config,
        renderer,
        scaler,
        scaler_scale,
        step: FixedStep::new(TICK),
        snaps: Snapshots::default(),
        canvas,
        hidden: false,
        fps: Default::default(),
        presented: None,
        frame_ms: 0.0,
    }));
    // requestAnimationFrame, each callback scheduling the next.
    let tick: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let next = tick.clone();
    *tick.borrow_mut() = Some(Closure::new(move || {
        let mut w = web.borrow_mut();
        w.frame();
        if !w.game.running() {
            if let Some(a) = &w.audio {
                a.borrow_mut().set_paused(true);
            }
            if let Some(f) = &on_quit {
                let _ = f.call0(&JsValue::NULL);
            }
            return;
        }
        if let Some(window) = web_sys::window() {
            let _ = window.request_animation_frame(next.borrow().as_ref().unwrap().as_ref().unchecked_ref());
        }
    }));
    let window = web_sys::window().ok_or_else(|| js_err("no window"))?;
    window.request_animation_frame(tick.borrow().as_ref().unwrap().as_ref().unchecked_ref()).map_err(|_| js_err("requestAnimationFrame"))?;
    Ok(())
}
