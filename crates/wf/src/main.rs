//! Warning Forever, ported to Rust.
//!
//! Layers, top to bottom:
//! * `game`: the port of the game (Application loop, tasks, stages, text,
//!   pads, sound system). Platform independent.
//! * seams: `gfx::DrawList` (what to draw), `platform::{RawInput, Event}`
//!   (what the player does), `sound::Mixer` (what to play).
//! * backends: `gfx::gpu` (wgpu, Slug rendering of the cooked art) and
//!   `platform::sdl` (SDL3 window, keyboard, gamepads, audio).

// The port keeps the decomp's shapes: its argument lists, index loops over
// parallel tables, constants written as the original's literals and
// negated float comparisons (which differ from their opposites on NaN).
#![allow(
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::excessive_precision,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::type_complexity
)]

// Much of the game-facing API is ported ahead of the stages that use it.
#![allow(dead_code)]

mod art;
mod data;
mod game;
mod shape_table;

pub use wf_engine::{audio as sound, gfx, log, platform};

use std::path::PathBuf;

use game::assets::{Assets, CHANNELS};
use game::{App, Cx};
use platform::{Key, RawInput};

struct Options {
    data: PathBuf,
    headless: bool,
    frames: u32,
    screenshot: Option<PathBuf>,
    /// The window (or headless image) size; the config's otherwise.
    size: Option<(u32, u32)>,
    fps_cap: Option<wf_engine::time::FpsCap>,
    window_mode: Option<wf_engine::config::WindowMode>,
    show_fps: bool,
    /// The arena of new games (else the last choice).
    arena: Option<game::arena::Mode>,
    /// Measure the uncapped frame rate for this many seconds (headless).
    bench: Option<f64>,
    /// Headless input: keys held on given frames.
    /// Keys held over frame ranges (FRAME or FIRST-LAST, inclusive).
    script: Vec<(u32, u32, Key)>,
    /// Where wf.ini, wf.sco and rec/ live.
    home: PathBuf,
    /// Start in the art viewer instead of the title.
    viewer: bool,
    /// Play this replay (headless checks) instead of the title.
    replay: Option<String>,
    hash_log: Option<PathBuf>,
}

fn parse_key(s: &str) -> Option<Key> {
    Some(match s.to_ascii_lowercase().as_str() {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "z" => Key::Z,
        "x" => Key::X,
        "d" => Key::D,
        "f" => Key::F,
        "esc" => Key::Escape,
        "f12" => Key::F12,
        "q" => Key::Q,
        "e" => Key::E,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        _ => return None,
    })
}

fn usage() -> ! {
    eprintln!(
        "usage: wf [--data <data.bin>] [--home <dir>] [--size WxH] [--frames N --screenshot out.png]\n          [--fps-cap off|vsync|N] [--fullscreen|--borderless|--windowed] [--show-fps] [--arena classic|hd]\n          [--headless [--bench SECONDS] --script FRAME[-LAST]:KEY,...] [--replay rec/<file>.rec] [--hash-log out.txt] [--viewer]\n\
         data.bin also comes from WF_DATA, else ./data.bin, else next to the executable.\n\
         The settings, scores and replays are kept in --home (WF_HOME, default: the current directory)."
    );
    std::process::exit(2);
}

fn options() -> Options {
    let mut o = Options {
        data: std::env::var_os("WF_DATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data.bin")),
        headless: false,
        frames: 60,
        screenshot: None,
        size: None,
        fps_cap: None,
        window_mode: None,
        show_fps: false,
        bench: None,
        arena: None,
        script: Vec::new(),
        home: std::env::var_os("WF_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")),
        viewer: false,
        replay: None,
        hash_log: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage());
        match a.as_str() {
            "--data" => o.data = value().into(),
            "--headless" => o.headless = true,
            "--home" => o.home = value().into(),
            "--viewer" => o.viewer = true,
            "--replay" => o.replay = Some(value()),
            "--hash-log" => o.hash_log = Some(value().into()),
            "--frames" => o.frames = value().parse().unwrap_or_else(|_| usage()),
            "--screenshot" => o.screenshot = Some(value().into()),
            "--size" => {
                let v = value();
                let (w, h) = v.split_once('x').unwrap_or_else(|| usage());
                o.size = Some((w.parse().unwrap_or_else(|_| usage()), h.parse().unwrap_or_else(|_| usage())));
            }
            "--fps-cap" => o.fps_cap = Some(wf_engine::time::FpsCap::parse(&value()).unwrap_or_else(|| usage())),
            "--fullscreen" => o.window_mode = Some(wf_engine::config::WindowMode::Fullscreen),
            "--borderless" => o.window_mode = Some(wf_engine::config::WindowMode::Borderless),
            "--windowed" => o.window_mode = Some(wf_engine::config::WindowMode::Windowed),
            "--show-fps" => o.show_fps = true,
            "--arena" => {
                o.arena = Some(match value().as_str() {
                    "classic" => game::arena::Mode::Classic,
                    "hd" => game::arena::Mode::Hd,
                    _ => usage(),
                })
            }
            "--bench" => o.bench = Some(value().parse().unwrap_or_else(|_| usage())),
            "--script" => {
                for item in value().split(',') {
                    let (f, k) = item.split_once(':').unwrap_or_else(|| usage());
                    let (a, b) = f.split_once('-').unwrap_or((f, f));
                    let n = |s: &str| s.parse().unwrap_or_else(|_| usage());
                    o.script.push((n(a), n(b), parse_key(k).unwrap_or_else(|| usage())));
                }
            }
            _ => usage(),
        }
    }
    if !o.data.exists()
        && let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
            let beside = exe_dir.join("data.bin");
            if beside.exists() {
                o.data = beside;
            }
        }
    o
}

/// cGlobal::Initialize and WinMain's setup: everything the stages share.
fn startup(o: &Options) -> Result<(Cx, App), String> {
    let assets = Assets::load(&o.data)?;
    let mut cx = Cx::new(art::Art::load(), assets, CHANNELS);
    game::time::set_utc_offset(platform::sdl::local_utc_offset());
    cx.open_home(o.home.clone());
    if let Some(path) = &o.hash_log {
        game::debug::start_hash_log(&mut cx, path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let (reset, first): (fn() -> Box<dyn game::app::Stage>, Box<dyn game::app::Stage>) = if o.viewer {
        (|| Box::new(game::viewer::Viewer::new()), Box::new(game::viewer::Viewer::new()))
    } else if let Some(path) = &o.replay {
        game::fn_game::FnGameState::set_mode(&mut cx, 2, Some(path));
        (|| Box::new(game::fn_title::FnTitle::new()), Box::new(game::fn_game::FnGame::new()))
    } else {
        (|| Box::new(game::fn_title::FnTitle::new()), Box::new(game::fn_title::FnTitle::new()))
    };
    let mut app = App::new(Some(reset));
    if !app.start(&mut cx, first) {
        return Err("the first stage could not start".into());
    }
    Ok((cx, app))
}

fn save_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut w| w.write_image_data(rgba)).map_err(|e| e.to_string())
}

/// The game as the engine's loop drives it: the simulation (`Cx` and the
/// stage loop) plus the input adapter in front of it.
struct WfGame {
    cx: Cx,
    app: App,
    /// The trigger maps last given to the bindings.
    synced: Option<([Key; 8], [u8; 8])>,
}

impl WfGame {
    fn ship(&self) -> Option<[f32; 2]> {
        let p = self.cx.game.player?;
        let a = self.cx.actors.get(p);
        Some([a.x(), a.y()])
    }
}

impl wf_engine::app::Game for WfGame {
    fn tick(&mut self, actions: &wf_engine::input::ActionState) -> bool {
        // InputTask::IsProtected (0x405f30): the pads are polled only while
        // the window is active; otherwise their buttons and presses hold.
        if self.active() {
            self.cx.input.steer.ship = self.ship();
            self.cx.input.poll(actions);
            self.cx.ui.update(actions);
        }
        let made = self.app.frame(&mut self.cx);
        if made && self.cx.settings.display.show_fps {
            // SHOW FPS: the display rate, bottom right (display only).
            let text = format!("{:.0} FPS", self.cx.settings.fps);
            let x = self.cx.arena.width - 8.0 * text.len() as f32 - 6.0;
            let y = self.cx.arena.height - 14.0;
            self.cx.fonts.small_text.print(&mut self.cx.draw, x, y, text.as_bytes(), None, gfx::Argb(0xa0ff_ffff), -1, gfx::Blend::Normal);
        }
        made
    }
    fn set_fps(&mut self, fps: f32) {
        self.cx.settings.fps = fps;
    }
    fn display_request(&mut self) -> Option<wf_engine::config::DisplayConfig> {
        let s = &mut self.cx.settings;
        std::mem::take(&mut s.display_dirty).then_some(s.display)
    }
    fn snapshot(&self) -> &gfx::DrawList {
        &self.cx.draw
    }
    fn running(&self) -> bool {
        self.app.running()
    }
    fn quit(&mut self) {
        self.app.quit(&mut self.cx);
    }
    fn focus(&mut self, active: bool) {
        if active {
            self.cx.app.flags &= !game::AppFlags::INACTIVE;
        } else {
            self.cx.app.flags |= game::AppFlags::INACTIVE;
        }
    }
    fn active(&self) -> bool {
        self.cx.app.flags & game::AppFlags::INACTIVE == 0
    }
    fn shader_time(&self) -> f32 {
        self.cx.app.frame_count as f32 / 58.8
    }
    fn sync_bindings(&mut self, b: &mut wf_engine::input::Bindings) {
        if std::mem::take(&mut self.cx.settings.input_dirty) {
            self.cx.settings.apply_input(b);
        }
        let pad = &self.cx.input.pads[0];
        let maps = (pad.key_map, pad.button_map);
        if self.synced == Some(maps) {
            return;
        }
        for i in 0..8 {
            b.set_trigger_key(i as u8, maps.0[i]);
            if let Some(&button) = platform::PadButton::TRIGGERS.get(maps.1[i] as usize) {
                b.set_trigger_button(i as u8, button);
            }
        }
        self.synced = Some(maps);
    }
    fn set_window_arena(&mut self, arena: [f32; 2]) {
        self.cx.window_arena_width = arena[0];
    }
    fn wants_border(&self) -> bool {
        self.cx.arena.is_classic()
    }
}

fn script_input(o: &Options, frame: u32) -> RawInput {
    let mut raw = RawInput::default();
    for &(first, last, k) in &o.script {
        if (first..=last).contains(&frame) {
            raw.keys[k as usize] = true;
        }
    }
    raw
}

/// Runs `frames` ticks without a window and saves the last one; or, with
/// --bench, measures the uncapped frame rate offscreen.
fn run_headless(o: &Options) -> Result<(), String> {
    let (cx, app) = startup(o)?;
    let mut game = WfGame { cx, app, synced: None };
    let size = o.size.unwrap_or((1280, 720));
    game.cx.window_arena_width = wf_engine::app::window_arena(size)[0];
    if let Some(mode) = o.arena {
        game.cx.arena_choice = mode;
    }
    let cfg = game.cx.port.clone();
    let mut input = wf_engine::input::InputSystem::new(wf_engine::input::Bindings::from_config(&cfg));
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let (_adapter, device, queue) = gfx::gpu::create_device(&instance, None)?;
    let mut renderer = gfx::gpu::Renderer::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, &game.cx.art.pack);
    let mut snaps = wf_engine::app::run_ticks(&mut game, &mut input, o.frames, |f| script_input(o, f));
    if let Some(seconds) = o.bench {
        let (fps, ticks) = wf_engine::app::bench(&mut game, &mut renderer, size, seconds, &mut input);
        println!("bench: {fps:.0} frames/s uncapped at {}x{} ({ticks} ticks in {seconds} s)", size.0, size.1);
        return Ok(());
    }
    if let Some(path) = &o.screenshot {
        renderer.time = game.cx.app.frame_count as f32 / 58.8;
        let layout = gfx::Layout::new(snaps.cur.arena, size);
        let frame = snaps.frame(1.0, &layout, true);
        let rgba = renderer.render_to_rgba(frame, size.0, size.1);
        save_png(path, size.0, size.1, &rgba)?;
        println!("saved {}", path.display());
    }
    Ok(())
}

/// The window icon (assets/icon), as RGBA.
fn icon() -> Option<(u32, u32, Vec<u8>)> {
    const PNG: &[u8] = include_bytes!("../../../assets/icon/windows/wf_256.png");
    let mut decoder = png::Decoder::new(std::io::Cursor::new(PNG));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some((info.width, info.height, buf))
}

fn run_windowed(o: &Options) -> Result<(), String> {
    let (cx, app) = startup(o)?;
    let mut game = WfGame { cx, app, synced: None };
    if let Some(mode) = o.arena {
        game.cx.arena_choice = mode;
    }
    let cfg = game.cx.port.clone();
    let mut display_cfg = wf_engine::config::DisplayConfig::from_config(&cfg);
    if let Some((w, h)) = o.size {
        (display_cfg.width, display_cfg.height) = (w, h);
    }
    if let Some(cap) = o.fps_cap {
        display_cfg.fps_cap = cap;
    }
    if let Some(mode) = o.window_mode {
        display_cfg.mode = mode;
    }
    display_cfg.show_fps |= o.show_fps;
    game.cx.settings.display = display_cfg;
    let mut platform = platform::sdl::Platform::new("WARNING FOREVER", display_cfg.width, display_cfg.height)?;
    wf_engine::app::apply_window_mode(&mut platform, display_cfg.mode);
    if let Some((w, h, rgba)) = icon() {
        platform.set_icon(w, h, &rgba);
    }
    let mut display = wf_engine::app::Display::new(&platform, &game.cx.art.pack, display_cfg.fps_cap)?;
    platform.start_audio(game.cx.sound.mixer());
    let mut input = wf_engine::input::InputSystem::new(wf_engine::input::Bindings::from_config(&cfg));
    let mut screenshot_taken = false;
    let frames = o.frames;
    let shot = o.screenshot.clone();
    let counter = std::cell::Cell::new(0u32);
    wf_engine::app::run_windowed(&mut game, &mut platform, &mut display, display_cfg, &mut input, |display, frame| {
        counter.set(counter.get() + 1);
        if let (Some(path), false) = (&shot, screenshot_taken)
            && counter.get() >= frames
        {
            let (w, h) = display.size();
            let rgba = display.renderer.render_to_rgba(frame, w, h);
            save_png(path, w, h, &rgba)?;
            screenshot_taken = true;
        }
        Ok(())
    })?;
    drop(display);
    Ok(())
}

fn main() {
    log::init_from_env();
    let o = options();
    let result = if o.headless { run_headless(&o) } else { run_windowed(&o) };
    if let Err(e) = result {
        // Shown like the original's Shutdown() MessageBox, on stderr.
        eprintln!("ERROR: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_icon_decodes() {
        let (w, h, rgba) = super::icon().expect("assets/icon/windows/wf_256.png");
        assert_eq!((w, h, rgba.len()), (256, 256, 256 * 256 * 4));
    }
}
