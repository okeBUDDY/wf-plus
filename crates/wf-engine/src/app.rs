//! The app loop: a fixed-step simulation under a free-running display.
//!
//! Each display frame the loop handles the window's events, runs the ticks
//! the accumulator says are due (each reading the actions of that moment),
//! then shows the last two ticks' render snapshots blended by the
//! accumulator's remainder, and waits as the frame cap says. The game is a
//! [`Game`]: deterministic state that turns actions into snapshots.

use std::time::Instant;

use crate::config::{DisplayConfig, WindowMode};
use crate::gfx::{self, DrawList, Layout};
use crate::input::{ActionState, Bindings, InputSystem};
use crate::platform::{self, Event, RawInput};
use crate::time::{FixedStep, FpsCap, FpsCounter, Limiter, TICK};

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
    /// Draw the subtle arena border when there are margins.
    fn wants_border(&self) -> bool {
        false
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

/// The window's surface and the renderer.
pub struct Display {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    caps: wgpu::SurfaceCapabilities,
    pub renderer: gfx::gpu::Renderer,
    /// Render target pixels per window pixel.
    pub render_scale: f32,
    scaler: gfx::scale::Scaler,
}

impl Display {
    pub fn new(platform: &platform::sdl::Platform, art: &wf_art::Pack, cap: FpsCap) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(platform.display_handle()?)));
        // SAFETY: the window outlives the surface: the Display is dropped
        // before the Platform.
        let surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&platform.window).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?
        };
        let (adapter, device, queue) = gfx::gpu::create_device(&instance, Some(&surface))?;
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
        surface.configure(&device, &config);
        let renderer = gfx::gpu::Renderer::new(device, queue, format, art);
        let scaler = gfx::scale::Scaler::new(&renderer);
        Ok(Display { surface, config, caps, renderer, render_scale: 1.0, scaler })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.config.width = w.max(1);
        self.config.height = h.max(1);
        self.surface.configure(self.renderer.device(), &self.config);
    }

    /// Mailbox or Immediate uncapped (and under the limiter), Fifo for
    /// VSync.
    pub fn set_cap(&mut self, cap: FpsCap) {
        let mode = present_mode(&self.caps, cap);
        if mode != self.config.present_mode {
            self.config.present_mode = mode;
            self.surface.configure(self.renderer.device(), &self.config);
        }
    }

    pub fn present(&mut self, list: &DrawList) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(self.renderer.device(), &self.config);
                return;
            }
            _ => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let size = (self.config.width, self.config.height);
        self.scaler.render(&mut self.renderer, list, &view, size, self.render_scale);
        self.renderer.queue().present(frame);
    }
}

fn present_mode(caps: &wgpu::SurfaceCapabilities, cap: FpsCap) -> wgpu::PresentMode {
    if cap == FpsCap::VSync {
        return wgpu::PresentMode::Fifo;
    }
    [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
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
    display.render_scale = cfg.render_scale;
    while game.running() {
        for event in platform.poll() {
            match event {
                Event::Quit => game.quit(),
                Event::Resized(w, h) => display.resize(w, h),
                Event::Focus(active) => {
                    game.focus(active);
                    step.reset();
                }
            }
        }
        if let Some(d) = game.display_request() {
            if d.mode != cfg.mode {
                platform.set_window_mode(d.mode);
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
        display.present(frame);
        on_frame(display, frame)?;
        let now = Instant::now();
        fps.frame(now);
        game.set_fps(fps.fps as f32);
        if cfg.show_fps && now.duration_since(title_at).as_secs_f32() > 0.5 {
            let title = format!("{} - {:.0} fps", platform.title, fps.fps);
            platform.set_title(&title);
            title_at = now;
        }
        limiter.wait(cfg.fps_cap);
    }
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
pub fn apply_window_mode(platform: &mut platform::sdl::Platform, mode: WindowMode) {
    if mode != WindowMode::Windowed {
        platform.set_window_mode(mode);
    }
}
