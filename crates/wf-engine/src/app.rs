//! The app loop: a fixed-step simulation under a free-running display.
//!
//! Each display frame the loop handles the window's events, runs the ticks
//! the accumulator says are due (each reading the actions of that moment),
//! then shows the last two ticks' render snapshots blended by the
//! accumulator's remainder, and waits as the frame cap says. The game is a
//! [`Game`]: deterministic state that turns actions into snapshots.

use web_time::Instant;

use crate::config::DisplayConfig;
use crate::gfx::{self, DrawList, Layout};
use crate::input::{ActionState, Bindings, InputSystem};
use crate::platform::RawInput;
use crate::time::{FixedStep, TICK};
// The window (SDL3): everywhere but the browser, whose loop is the page's.
#[cfg(not(target_arch = "wasm32"))]
use crate::config::WindowMode;
#[cfg(not(target_arch = "wasm32"))]
use crate::platform::{self, Event};
#[cfg(not(target_arch = "wasm32"))]
use crate::time::{FpsCap, FpsCounter, Limiter};

/// What the loop drives.
pub trait Game {
    /// One simulation tick on `actions`. Returns true when it made a new
    /// render snapshot (false while paused).
    fn tick(&mut self, actions: &ActionState) -> bool;
    /// The last tick's render snapshot.
    fn snapshot(&self) -> &DrawList;
    fn running(&self) -> bool;
    fn quit(&mut self);
    /// The window got (true) or lost the focus.
    fn focus(&mut self, active: bool);
    /// Input is read only while active (InputTask::IsProtected).
    fn active(&self) -> bool;
    /// Seconds of game time, for animated paints.
    fn shader_time(&self) -> f32;
    /// The arena the window would give a game started now (logical units).
    fn set_window_arena(&mut self, _arena: [f32; 2]) {}
    /// The window's size in pixels (what the frames are laid out to).
    fn set_window_size(&mut self, _size: (u32, u32)) {}
    /// Lets the game's own remaps (the options screen) reach the bindings.
    fn sync_bindings(&mut self, _bindings: &mut Bindings) {}
    /// The display settings changed by the game (its options screen).
    fn display_request(&mut self) -> Option<DisplayConfig> {
        None
    }
    /// The measured display rate, once a frame (the FPS counter).
    fn set_fps(&mut self, _fps: f32) {}
    /// The GPU time of a frame (ms, from timestamp queries; None when the
    /// adapter has none) and the present-to-present time (ms, averaged),
    /// once a frame (the perf overlay).
    fn set_frame_time(&mut self, _gpu_ms: Option<f32>, _frame_ms: f32) {}
    /// A file was dropped on the window, or chosen in the file dialog.
    fn file_dropped(&mut self, _path: std::path::PathBuf) {}
    /// True to open the system's file dialog (for a zip); the chosen file
    /// arrives as [`Game::file_dropped`].
    fn file_dialog(&mut self) -> bool {
        false
    }
    /// Draw the subtle arena border when there are margins.
    fn wants_border(&self) -> bool {
        false
    }
    /// The app went to the background (true) or came back (mobile). The
    /// loop stops ticking meanwhile; a game may also pause itself.
    fn background(&mut self, on: bool) {
        self.focus(!on);
    }
}

/// The arena border's colour: a muted indigo-violet (#7a5cff) at low
/// alpha. The game's palette is green and teal (grid, bosses), amber (HUD),
/// white and grey (text), crimson (cores) and yellow to orange
/// (explosions); violet is used nowhere, sits opposite the greens, and
/// stays clear of the crimson.
pub const ARENA_BORDER: u32 = 0x607a_5cff;

/// The arena border: a thin low-alpha stroke, display only, after the
/// background and before the arena's draws. It is drawn only on the sides
/// where the view extends past the arena, half a logical pixel outside the
/// arena so the 1 px stroke sits entirely in the margin.
pub fn add_border(list: &mut DrawList, layout: &Layout) {
    let [x, y, w, h] = layout.visible;
    let [aw, ah] = list.arena;
    let (left, top) = (x < -0.5, y < -0.5);
    let (right, bottom) = (x + w > aw + 0.5, y + h > ah + 0.5);
    if !(left || top || right || bottom) {
        return;
    }
    let at = list.draws.iter().position(|d| d.pass() == gfx::Pass::Arena).unwrap_or(list.draws.len());
    let mut tail = list.draws.split_off(at);
    list.set_pass(gfx::Pass::Background);
    let (x0, y0, x1, y1) = (-0.5, -0.5, aw + 0.5, ah + 0.5);
    let sides = [
        (left, [[x0, y0], [x0, y1]]),
        (right, [[x1, y0], [x1, y1]]),
        (top, [[x0, y0], [x1, y0]]),
        (bottom, [[x0, y1], [x1, y1]]),
    ];
    for (on, pts) in sides {
        if on {
            list.shape(gfx::Primitive::LineStrip, &gfx::Transform::IDENTITY, &pts, gfx::Argb(ARENA_BORDER), gfx::Blend::Normal);
        }
    }
    list.set_pass(gfx::Pass::Arena);
    list.draws.append(&mut tail);
}

#[cfg(not(target_arch = "wasm32"))]
/// The window's surface and the renderer.
///
/// Mobile windows lose their native surface when the app goes to the
/// background, and on Android the Java thread releases it
/// (surfaceDestroyed) without waiting for the game thread. So each frame
/// [`Display::present`] checks, under [`Platform::lock_surface`] (SDL's
/// activity mutex there), that the native window is still the one the
/// surface was made for; the surface is dropped when the window is gone or
/// changed, and made again (lazily, under the lock) when there is one. The
/// lock is held only for that: a VkSurfaceKHR keeps its own reference to
/// the ANativeWindow, and holding the lock across acquire and present can
/// deadlock with the Java UI thread, which takes it in surfaceChanged.
///
/// [`Platform::lock_surface`]: platform::sdl::Platform::lock_surface
pub struct Display {
    instance: wgpu::Instance,
    /// None while the window has no native surface, or after it was lost.
    surface: Option<wgpu::Surface<'static>>,
    /// The native window `surface` was made for.
    surface_window: usize,
    /// The surface needs (re)configuring before the next acquire.
    stale: bool,
    /// The app is in the background: no surface until it is back.
    background: bool,
    config: wgpu::SurfaceConfiguration,
    caps: wgpu::SurfaceCapabilities,
    pub renderer: gfx::gpu::Renderer,
    /// Render target pixels per window pixel.
    pub render_scale: f32,
    scaler: gfx::scale::Scaler,
    /// Frames presented so far.
    presented: u64,
    /// The first acquire was logged.
    acquiring: bool,
    /// Frames in a row not shown (logged now and then).
    misses: u64,
}

/// The first uncaptured GPU error (see `Display::new`).
static GPU_ERROR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Frames presented by any Display (the Android startup watchdog).
#[cfg(not(target_arch = "wasm32"))]
static PRESENTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The first uncaptured GPU error, taken.
pub fn take_gpu_error() -> Option<String> {
    GPU_ERROR.lock().ok().and_then(|mut e| e.take())
}

#[cfg(not(target_arch = "wasm32"))]
impl Display {
    pub fn new(platform: &platform::sdl::Platform, art: &wf_art::Pack, cap: FpsCap) -> Result<Self, String> {
        #[allow(unused_mut)]
        let mut desc = wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(platform.display_handle()?));
        #[cfg(target_os = "android")]
        if let Some(b) = platform::android::backends() {
            desc.backends = b;
        }
        crate::log::breadcrumb(&format!("GPU instance: backends {:?}", desc.backends));
        let instance = wgpu::Instance::new(desc);
        // The lock only covers reading the native window and making the
        // surface on it: the VkSurfaceKHR holds its own reference to the
        // ANativeWindow, so nothing after this can touch a freed window
        // (a dead one gives surface errors, handled in `present`). Device
        // and pipeline creation, configure, acquire and present all run
        // without it, so the Java UI thread (surfaceChanged and
        // surfaceDestroyed take the same lock) never waits on the GPU.
        let (surface, window) = {
            let lock = platform.lock_surface();
            if lock.window == 0 {
                return Err("the window has no native surface".into());
            }
            (create_surface(&instance, platform)?, lock.window)
        };
        crate::log::breadcrumb(&format!("surface made for window {window:#x}"));
        let (adapter, device, queue) = gfx::gpu::create_device(&instance, Some(&surface))?;
        // GPU errors are logged and kept: the loop stops with the first
        // one (a message box on Android) instead of showing black frames.
        // Surface errors go through error scopes or `CurrentSurfaceTexture`
        // and never get here.
        device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
            let msg = format!("wgpu: {e}");
            crate::log::error(&msg);
            if let Ok(mut first) = GPU_ERROR.lock() {
                first.get_or_insert(msg);
            }
        }));
        let info = adapter.get_info();
        let caps = surface.get_capabilities(&adapter);
        // Blend in gamma space, as the original's Direct3D 7 did.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or("the window surface has no format")?;
        let (w, h) = platform.drawable_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: w.max(1),
            height: h.max(1),
            present_mode: present_mode(&caps, cap),
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: Default::default(),
        };
        crate::log::breadcrumb(&format!(
            "surface caps: formats {:?}, present modes {:?}; chosen {:?}, {:?}, {}x{}; building the renderer",
            caps.formats, caps.present_modes, format, config.present_mode, config.width, config.height
        ));
        let scope_oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let renderer = gfx::gpu::Renderer::new(device, queue, format, art);
        crate::log::breadcrumb("scaler");
        let scaler = gfx::scale::Scaler::new(&renderer);
        crate::log::breadcrumb("waiting for the renderer's GPU errors");
        if let Some(e) = pollster::block_on(scope.pop()).or(pollster::block_on(scope_oom.pop())) {
            return Err(format!("the GPU renderer cannot be made on {} ({:?}): {e}", info.name, info.backend));
        }
        crate::log::breadcrumb("renderer ready");
        let mut display = Display {
            instance,
            surface: Some(surface),
            surface_window: window,
            stale: true,
            background: false,
            config,
            caps,
            renderer,
            render_scale: 1.0,
            scaler,
            presented: 0,
            acquiring: false,
            misses: 0,
        };
        if !display.configure() {
            return Err("cannot configure the window surface".into());
        }
        crate::log::breadcrumb("surface configured");
        Ok(display)
    }

    /// The app is going to the background: drops the surface and makes
    /// none until [`Display::resume`].
    pub fn suspend(&mut self) {
        BACKGROUND.store(true, std::sync::atomic::Ordering::Relaxed);
        self.background = true;
        if self.surface.take().is_some() {
            crate::log::breadcrumb("surface dropped (background)");
        }
    }

    /// Back in the foreground: the next present makes a new surface once
    /// the window has a native one.
    pub fn resume(&mut self) {
        self.background = false;
        BACKGROUND.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn suspended(&self) -> bool {
        self.background
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// The window was resized: reconfigured at the next present.
    pub fn resize(&mut self, w: u32, h: u32) {
        self.config.width = w.max(1);
        self.config.height = h.max(1);
        self.stale = true;
    }

    /// Mailbox or Immediate uncapped (and under the limiter), Fifo for
    /// VSync (Android: Mailbox or Fifo). Applied at the next present.
    pub fn set_cap(&mut self, cap: FpsCap) {
        let mode = present_mode(&self.caps, cap);
        if mode != self.config.present_mode {
            self.config.present_mode = mode;
            self.stale = true;
        }
    }

    /// Configures the surface; false (and the surface dropped) on error.
    fn configure(&mut self) -> bool {
        let Some(surface) = &self.surface else { return false };
        if self.presented == 0 {
            crate::log::breadcrumb(&format!(
                "configure: {:?}, {:?}, {}x{}",
                self.config.format, self.config.present_mode, self.config.width, self.config.height
            ));
        }
        let device = self.renderer.device();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        surface.configure(device, &self.config);
        if let Some(e) = pollster::block_on(scope.pop()) {
            crate::log::breadcrumb(&format!("surface configure failed, dropping it: {e}"));
            self.surface = None;
            return false;
        }
        self.stale = false;
        true
    }

    /// A frame not shown for `why`: a breadcrumb for the first and every
    /// 120th in a row, so a loop that never shows one says why.
    fn missed(&mut self, why: &str) {
        if self.misses.is_multiple_of(120) {
            crate::log::breadcrumb(&format!("frame not shown: {why} ({} in a row)", self.misses + 1));
        }
        self.misses += 1;
    }

    /// Shows `list`, if the window has a surface to show it on. The
    /// surface is checked and made here, under the platform's surface lock.
    pub fn present(&mut self, platform: &platform::sdl::Platform, list: &DrawList) {
        if self.background {
            return;
        }
        {
            // Held only to read the native window and make a surface on it
            // (see `new`); released before any GPU call that can wait.
            let lock = platform.lock_surface();
            if lock.window == 0 {
                if self.surface.take().is_some() {
                    crate::log::breadcrumb("native window gone: surface dropped");
                }
                self.missed("no native window");
                return;
            }
            if self.surface.is_some() && self.surface_window != lock.window {
                crate::log::breadcrumb(&format!("native window changed {:#x} -> {:#x}: surface dropped", self.surface_window, lock.window));
                self.surface = None;
            }
            if self.surface.is_none() {
                match create_surface(&self.instance, platform) {
                    Ok(s) => {
                        let (w, h) = platform.drawable_size();
                        (self.config.width, self.config.height) = (w.max(1), h.max(1));
                        crate::log::breadcrumb(&format!("surface created for window {:#x}, {w}x{h}", lock.window));
                        self.surface = Some(s);
                        self.surface_window = lock.window;
                        self.stale = true;
                    }
                    Err(e) => {
                        crate::log::breadcrumb(&format!("cannot create the surface yet: {e}"));
                        return;
                    }
                }
            }
        }
        if self.stale && !self.configure() {
            return;
        }
        let Some(surface) = &self.surface else { return };
        if self.presented == 0 && !self.acquiring {
            self.acquiring = true;
            crate::log::breadcrumb("first acquire");
        }
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                self.stale = true;
                f
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                crate::log::breadcrumb("surface outdated: reconfiguring");
                self.stale = true;
                return;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                crate::log::breadcrumb("surface lost: dropped, made again next frame");
                self.surface = None;
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                crate::log::breadcrumb("surface acquire: validation error, reconfiguring");
                self.stale = true;
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                self.missed("acquire timed out");
                return;
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                self.missed("surface occluded");
                return;
            }
        };
        if self.presented == 0 {
            crate::log::breadcrumb("first acquire: got a frame; rendering");
        }
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let size = (self.config.width, self.config.height);
        self.scaler.render(&mut self.renderer, list, &view, size, self.render_scale);
        if self.presented == 0 {
            crate::log::breadcrumb("first frame submitted; presenting");
        }
        self.renderer.queue().present(frame);
        self.presented += 1;
        self.misses = 0;
        if self.presented == 1 {
            crate::log::breadcrumb("first frame presented");
            #[cfg(target_os = "android")]
            platform::android::first_frame();
        }
        PRESENTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// The window's surface. Called under the platform's surface lock, with a
/// native window present (sdl3's window handle does not check for one).
fn create_surface(instance: &wgpu::Instance, platform: &platform::sdl::Platform) -> Result<wgpu::Surface<'static>, String> {
    // SAFETY: the window outlives the surface: the Display is dropped
    // before the Platform, and on Android the surface is dropped as soon
    // as the native window it was made for is gone (see Display).
    unsafe {
        instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&platform.window).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn present_mode(caps: &wgpu::SurfaceCapabilities, cap: FpsCap) -> wgpu::PresentMode {
    if cap == FpsCap::VSync {
        return wgpu::PresentMode::Fifo;
    }
    // Android: Mailbox where offered, else Fifo (always there); Immediate
    // tears and is not offered by every driver.
    #[cfg(target_os = "android")]
    let modes = [wgpu::PresentMode::Mailbox];
    #[cfg(not(target_os = "android"))]
    let modes = [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate];
    modes
        .into_iter()
        .find(|m| caps.present_modes.contains(m))
        .unwrap_or(wgpu::PresentMode::Fifo)
}

/// The snapshots the display blends: the previous tick's and this one's.
#[derive(Default)]
pub struct Snapshots {
    pub prev: DrawList,
    pub cur: DrawList,
    shown: DrawList,
}

impl Snapshots {
    /// Takes the game's newest snapshot (or holds the last one when the
    /// tick made none).
    pub fn push(&mut self, made: bool, game: &dyn Game) {
        if made {
            std::mem::swap(&mut self.prev, &mut self.cur);
            self.cur.clone_from(game.snapshot());
        } else {
            self.prev.clone_from(&self.cur);
        }
    }

    /// The frame to show at `alpha`.
    pub fn frame(&mut self, alpha: f32, layout: &Layout, border: bool) -> &DrawList {
        gfx::interpolate(&self.prev, &self.cur, alpha, &mut self.shown);
        if border && !self.shown.menu {
            add_border(&mut self.shown, layout);
        }
        &self.shown
    }
}

/// Android: reports an app that shows no frame for [`Watchdog::STALL`]
/// while in the foreground, from start-up on (a hang that would otherwise
/// be a black screen): to logcat with the last breadcrumb, and once in a
/// message box. Runs until dropped.
#[cfg(target_os = "android")]
pub struct Watchdog {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(target_os = "android")]
impl Watchdog {
    pub const STALL: std::time::Duration = std::time::Duration::from_secs(15);
    /// No report this soon after a resume.
    pub const RESUME_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

    pub fn start() -> Self {
        use std::sync::atomic::Ordering;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        let spawned = std::thread::Builder::new().name("wf-watchdog".into()).spawn(move || {
            let mut seen = PRESENTED.load(Ordering::Relaxed);
            let mut since = Instant::now();
            // Frames flowing without a half-second gap since then.
            let mut flowing: Option<Instant> = None;
            let mut told = false;
            while !flag.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let now = PRESENTED.load(Ordering::Relaxed);
                if now != seen {
                    flowing.get_or_insert_with(Instant::now);
                } else {
                    flowing = None;
                }
                if crate::log::status_shown() {
                    crate::platform::android::status_tick(flowing.map(|t| t.elapsed()));
                }
                // Never while paused or in the background, nor in the first
                // seconds after a resume.
                let away = BACKGROUND.load(Ordering::Relaxed) || crate::platform::android::paused_or_resuming(Self::RESUME_GRACE);
                if now != seen || away {
                    (seen, since) = (now, Instant::now());
                    continue;
                }
                if since.elapsed() >= Self::STALL && !flag.load(Ordering::Relaxed) {
                    let steps = crate::log::game_steps();
                    crate::log::error(&format!("watchdog: no frame shown for {} s ({now} so far); game thread steps:\n{steps}", since.elapsed().as_secs()));
                    if !told {
                        told = true;
                        let header = format!(
                            "NO FRAME for {} s ({now} shown so far). Game thread's last steps (seconds since start; also in {}):",
                            Self::STALL.as_secs(),
                            crate::platform::android::status_path().with_file_name("wf-log.txt").display()
                        );
                        crate::platform::android::show_status(&header);
                        crate::platform::android::fatal(&format!("{header}\n{steps}"));
                    }
                    since = Instant::now();
                }
            }
        });
        if let Err(e) = spawned {
            crate::log::warn(&format!("watchdog not started: {e}"));
        }
        Watchdog { stop }
    }
}

#[cfg(target_os = "android")]
impl Drop for Watchdog {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// In the background (the watchdog waits for no frames then).
#[cfg(not(target_arch = "wasm32"))]
static BACKGROUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(not(target_arch = "wasm32"))]
/// The windowed loop. `on_frame` sees each shown frame (screenshots).
pub fn run_windowed(
    game: &mut dyn Game,
    platform: &mut platform::sdl::Platform,
    display: &mut Display,
    mut cfg: DisplayConfig,
    input: &mut InputSystem,
    mut on_frame: impl FnMut(&mut Display, &DrawList) -> Result<(), String>,
) -> Result<(), String> {
    let mut step = FixedStep::new(TICK);
    let mut limiter = Limiter::new();
    let mut fps = FpsCounter::default();
    let mut snaps = Snapshots::default();
    let mut title_at = Instant::now();
    let mut presented: Option<Instant> = None;
    let mut frame_ms = 0.0f32;
    display.render_scale = cfg.render_scale;
    crate::log::breadcrumb("loop started");
    while game.running() {
        for event in platform.poll() {
            match event {
                Event::Quit => game.quit(),
                Event::Resized(w, h) => {
                    crate::log::breadcrumb(&format!("resized {w}x{h}"));
                    display.resize(w, h);
                }
                Event::Focus(active) => {
                    game.focus(active);
                    step.reset();
                }
                Event::Background => {
                    crate::log::breadcrumb("background: game paused, audio paused, surface dropped");
                    game.background(true);
                    platform.set_audio_paused(true);
                    display.suspend();
                }
                Event::Foreground => {
                    crate::log::breadcrumb("foreground: surface made at the next present");
                    display.resume();
                    platform.set_audio_paused(false);
                    game.background(false);
                    step.reset();
                }
            }
        }
        if !game.running() {
            break;
        }
        // In the background (mobile): no ticks, no frames. On Android the
        // next poll blocks until the app comes back. The surface itself is
        // guarded in Display::present, whatever order the events come in.
        if display.suspended() {
            std::thread::sleep(std::time::Duration::from_millis(20));
            continue;
        }
        if game.file_dialog() {
            platform.open_file_dialog("Warning Forever 1.08 (wf108.zip, data.bin)", "zip;bin");
        }
        for path in platform.take_dropped() {
            game.file_dropped(path);
        }
        if let Some(d) = game.display_request() {
            if d.mode != cfg.mode || (d.mode == WindowMode::Fullscreen && d.fullscreen_size != cfg.fullscreen_size) {
                platform.set_window_mode(d.mode, d.fullscreen_size);
            }
            if d.mode == WindowMode::Windowed && (d.width, d.height) != (cfg.width, cfg.height) {
                platform.set_window_size(d.width, d.height);
            }
            display.set_cap(d.fps_cap);
            display.render_scale = d.render_scale;
            cfg = d;
        }
        let size = display.size();
        let window_arena = window_arena(size);
        game.set_window_arena(window_arena);
        game.set_window_size(size);
        let ticks = step.advance(Instant::now());
        for _ in 0..ticks {
            game.sync_bindings(&mut input.bindings);
            let layout = Layout::new(snaps.cur.arena, size);
            let actions = if game.active() { input.update(&platform.input(), &layout) } else { ActionState::default() };
            let made = game.tick(&actions);
            snaps.push(made, game);
            if !game.running() {
                break;
            }
        }
        display.renderer.time = game.shader_time();
        let layout = Layout::new(snaps.cur.arena, size);
        let border = game.wants_border();
        let frame = snaps.frame(step.alpha(), &layout, border);
        display.present(platform, frame);
        if let Some(e) = take_gpu_error() {
            return Err(format!("GPU error: {e}"));
        }
        on_frame(display, frame)?;
        let now = Instant::now();
        fps.frame(now);
        game.set_fps(fps.fps as f32);
        // Present to present: the CPU-side frame time (the fallback of the
        // GPU timestamps), smoothed.
        if let Some(at) = presented {
            let ms = now.duration_since(at).as_secs_f32() * 1e3;
            frame_ms = if frame_ms == 0.0 { ms } else { frame_ms + (ms - frame_ms) * 0.05 };
        }
        presented = Some(now);
        game.set_frame_time(display.renderer.gpu_ms(), frame_ms);
        if cfg.show_fps && now.duration_since(title_at).as_secs_f32() > 0.5 {
            let title = format!("{} - {:.0} fps", platform.title, fps.fps);
            platform.set_title(&title);
            title_at = now;
        }
        limiter.wait(cfg.fps_cap);
    }
    crate::log::breadcrumb("loop ended");
    Ok(())
}

/// The HD arena a window of `size` pixels gives: 480 high, as wide
/// as the window's aspect, kept within [`ARENA_WIDTH_RANGE`].
pub fn window_arena(size: (u32, u32)) -> [f32; 2] {
    let aspect = size.0.max(1) as f32 / size.1.max(1) as f32;
    let w = (480.0 * aspect).round().clamp(ARENA_WIDTH_RANGE.0, ARENA_WIDTH_RANGE.1);
    [w, 480.0]
}

/// The HD arena's width limits: 270 (9:16 portrait) to 1920 (4:1).
pub const ARENA_WIDTH_RANGE: (f32, f32) = (270.0, 1920.0);

/// Headless: runs `ticks` ticks on scripted input, as fast as possible.
pub fn run_ticks(game: &mut dyn Game, input: &mut InputSystem, ticks: u32, script: impl Fn(u32) -> RawInput) -> Snapshots {
    let mut snaps = Snapshots::default();
    for frame in 0..ticks {
        game.sync_bindings(&mut input.bindings);
        let layout = Layout::new(snaps.cur.arena, (640, 480));
        let actions = input.update(&script(frame), &layout);
        let made = game.tick(&actions);
        snaps.push(made, game);
        if !game.running() {
            break;
        }
    }
    snaps
}

/// Measures the uncapped display rate offscreen: real-time ticks and an
/// interpolated frame rendered (and waited for) each pass, for `seconds`.
/// Returns (frames per second, ticks run).
pub fn bench(game: &mut dyn Game, renderer: &mut gfx::gpu::Renderer, size: (u32, u32), seconds: f64, input: &mut InputSystem) -> (f64, u32) {
    let mut step = FixedStep::new(TICK);
    let mut snaps = Snapshots::default();
    let start = Instant::now();
    let (mut frames, mut ticks) = (0u32, 0u32);
    let idle = RawInput::default();
    while start.elapsed().as_secs_f64() < seconds && game.running() {
        for _ in 0..step.advance(Instant::now()) {
            let layout = Layout::new(snaps.cur.arena, size);
            let made = game.tick(&input.update(&idle, &layout));
            snaps.push(made, game);
            ticks += 1;
        }
        renderer.time = game.shader_time();
        let layout = Layout::new(snaps.cur.arena, size);
        let border = game.wants_border();
        let frame = snaps.frame(step.alpha(), &layout, border);
        renderer.render_offscreen(frame, size);
        frames += 1;
    }
    (frames as f64 / start.elapsed().as_secs_f64(), ticks)
}

/// Applies the window mode at start-up.
#[cfg(not(target_arch = "wasm32"))]
pub fn apply_window_mode(platform: &mut platform::sdl::Platform, d: &DisplayConfig) {
    if d.mode != WindowMode::Windowed {
        platform.set_window_mode(d.mode, d.fullscreen_size);
    }
}
