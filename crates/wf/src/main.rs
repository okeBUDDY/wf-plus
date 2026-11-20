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

mod app_info;
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
    /// Render target pixels per window pixel.
    render_scale: Option<f32>,
    /// The arena of new games (else the last choice).
    arena: Option<game::arena::Mode>,
    /// Always show the input guide, for this device (screenshots).
    guide: Option<platform::Device>,
    /// Another platform's display capabilities (the options' rows).
    caps: Option<platform::Capabilities>,
    /// Measure the uncapped frame rate for this many seconds (headless).
    bench: Option<f64>,
    /// Per-frame costs over --frames ticks, written as CSV (the perf bench).
    perf: Option<PathBuf>,
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
        "usage: wf [--data <data.bin>] [--home <dir>] [--size WxH] [--frames N --screenshot out.png]\n          [--fps-cap off|vsync|N] [--fullscreen|--borderless|--windowed] [--show-fps] [--render-scale K] [--arena classic|hd]\n          [--guide keyboard|gamepad|mouse|touch] [--caps desktop|android]\n          [--headless [--bench SECONDS] [--perf out.csv] --script FRAME[-LAST]:KEY,...] [--replay rec/<file>.rec] [--hash-log out.txt] [--viewer]\n\
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
        render_scale: None,
        bench: None,
        arena: None,
        guide: None,
        caps: None,
        perf: None,
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
            "--render-scale" => o.render_scale = Some(value().parse::<f32>().unwrap_or_else(|_| usage()).clamp(0.25, 4.0)),
            "--arena" => {
                o.arena = Some(match value().as_str() {
                    "classic" => game::arena::Mode::Classic,
                    "hd" => game::arena::Mode::Hd,
                    _ => usage(),
                })
            }
            "--guide" => {
                o.guide = Some(match value().as_str() {
                    "keyboard" => platform::Device::Keyboard,
                    "gamepad" => platform::Device::Gamepad,
                    "touch" => platform::Device::Touch,
                    "mouse" => platform::Device::Mouse,
                    _ => usage(),
                })
            }
            "--caps" => o.caps = Some(platform::Capabilities::parse(&value()).unwrap_or_else(|| usage())),
            "--perf" => o.perf = Some(PathBuf::from(value())),
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
    if let Some(caps) = o.caps {
        cx.settings.set_caps(caps);
    }
    if let Some(device) = o.guide {
        cx.settings.input_guide = game::guide::Policy::Always;
        cx.settings.guide_device = Some(device);
    }
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
            // A forced guide device (screenshots) is the one the menus show.
            self.cx.ui.shown = self.cx.settings.guide_device;
        }
        let made = self.app.frame(&mut self.cx);
        if made {
            // The input guide and SHOW FPS along the bottom (display only).
            game::guide::draw(&mut self.cx);
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
    fn set_window_size(&mut self, size: (u32, u32)) {
        self.cx.window_size = size;
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
    game.cx.settings.display.show_fps |= o.show_fps;
    let size = o.size.unwrap_or((1280, 720));
    game.cx.window_arena_width = wf_engine::app::window_arena(size)[0];
    game.cx.window_size = size;
    if let Some(mode) = o.arena {
        game.cx.arena_choice = mode;
    }
    let cfg = game.cx.port.clone();
    let mut input = wf_engine::input::InputSystem::new(wf_engine::input::Bindings::from_config(&cfg));
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let (_adapter, device, queue) = gfx::gpu::create_device(&instance, None)?;
    let mut renderer = gfx::gpu::Renderer::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, &game.cx.art.pack);
    if let Some(path) = &o.perf {
        return perf(o, &mut game, &mut renderer, &mut input, size, path);
    }
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
        let scale = o.render_scale.unwrap_or(game.cx.settings.display.render_scale);
        let mut scaler = gfx::scale::Scaler::new(&renderer);
        let rgba = renderer.render_to_rgba_with(size.0, size.1, |r, view| scaler.render(r, frame, view, size, scale));
        save_png(path, size.0, size.1, &rgba)?;
        println!("saved {}", path.display());
    }
    Ok(())
}

/// The perf bench: runs --frames ticks on the script (or a replay),
/// rendering and waiting for every one at `size`, and writes each frame's
/// costs to `path` as CSV; prints the means, the worst frames and the
/// busiest frame's counts.
fn perf(
    o: &Options,
    game: &mut WfGame,
    renderer: &mut gfx::gpu::Renderer,
    input: &mut wf_engine::input::InputSystem,
    size: (u32, u32),
    path: &std::path::Path,
) -> Result<(), String> {
    use std::fmt::Write as _;
    use wf_engine::app::Game;
    renderer.collect = true;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let (_adapter, device, queue) = gfx::gpu::create_device(&instance, None)?;
    let mut counter = gfx::gpu::Renderer::new_counting(device, queue, &game.cx.art.pack);
    let mut csv = String::from(
        "visits_m,frame,update_us,draw_us,build_us,dynamic_us,upload_us,submit_us,gpu_us,draws,instances,draw_calls,pipelines,upload_bytes,dynamic_rebuilt,quad_mpx,layer_mpx,curve_mpx,noise_mpx\n",
    );
    let mut rows: Vec<[f64; 17]> = Vec::new();
    // Render every `every`-th frame from `from` on (the GPU of a headless
    // run may be a slow software one).
    let every: u32 = std::env::var("WF_PERF_EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
    let from: u32 = std::env::var("WF_PERF_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut busiest: (f64, Vec<(u32, (u32, f64))>) = (0.0, Vec::new());
    let (mut visits_sum, mut visits_max) = (0.0f64, 0.0f64);
    for f in 0..o.frames {
        game.sync_bindings(&mut input.bindings);
        let layout = gfx::Layout::new(game.cx.draw.arena, size);
        let actions = input.update(&script_input(o, f), &layout);
        if !game.tick(&actions) || f < from || !(f - from).is_multiple_of(every) {
            if !game.running() {
                break;
            }
            continue;
        }
        renderer.time = game.cx.app.frame_count as f32 / 58.8;
        let t = std::time::Instant::now();
        renderer.render_offscreen(&game.cx.draw, size);
        let total = t.elapsed().as_secs_f64() * 1e6;
        let s = renderer.stats;
        counter.time = renderer.time;
        let visits = counter.count_visits(&game.cx.draw, size) / 1e6;
        visits_sum += visits;
        visits_max = visits_max.max(visits);
        let _ = write!(csv, "{visits:.2},");
        if s.curve_px > busiest.0 {
            let mut v: Vec<(u32, (u32, f64))> = renderer.by_sprite.iter().map(|(&k, &v)| (k, v)).collect();
            v.sort_by(|a, b| b.1.1.total_cmp(&a.1.1));
            busiest = (s.curve_px, v);
        }
        let gpu = total - s.build_us - s.upload_us - s.submit_us;
        let r = [
            f as f64,
            game.cx.app.update_us,
            game.cx.app.draw_us,
            s.build_us,
            s.dynamic_us,
            s.upload_us,
            s.submit_us,
            gpu,
            s.draws as f64,
            s.instances as f64,
            s.draw_calls as f64,
            s.pipeline_changes as f64,
            s.upload_bytes as f64,
            s.dynamic_rebuilt as f64,
            s.quad_px / 1e6,
            s.layer_px / 1e6,
            s.curve_px / 1e6,
        ];
        let _ = writeln!(csv, "{},{:.4}", r.map(|v| format!("{v:.1}")).join(","), s.noise_px / 1e6);
        rows.push(r);
        if !game.running() {
            break;
        }
    }
    std::fs::write(path, csv).map_err(|e| e.to_string())?;
    let n = rows.len().max(1) as f64;
    let names = ["update", "draw", "build", "dynamic", "upload", "submit", "gpu"];
    let mut line = format!("perf {}x{}, {} frames: mean us", size.0, size.1, rows.len());
    for (i, name) in names.iter().enumerate() {
        let _ = write!(line, " {name} {:.0}", rows.iter().map(|r| r[i + 1]).sum::<f64>() / n);
    }
    println!("{line}");
    println!("curve visits (measured): mean {:.1} M, max {:.1} M per frame", visits_sum / n, visits_max);
    let cpu = |r: &[f64; 17]| r[1] + r[2] + r[3] + r[5] + r[6];
    let mut worst: Vec<&[f64; 17]> = rows.iter().collect();
    worst.sort_by(|a, b| (cpu(b) + b[7]).total_cmp(&(cpu(a) + a[7])));
    for r in worst.iter().take(3) {
        println!(
            "worst frame {}: cpu {:.0} us (update {:.0} draw {:.0} build {:.0} dynamic {:.0} upload {:.0} submit {:.0}) gpu {:.0} us; draws {} instances {} calls {} pipelines {} upload {} B rebuilt {}; quad {:.2} Mpx layer {:.2} Mpx curve {:.1} Mpx",
            r[0], cpu(r), r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8], r[9], r[10], r[11], r[12], r[13], r[14], r[15], r[16]
        );
    }
    println!("busiest frame by sprite (curve Mpx, draws):");
    for (id, (count, px)) in busiest.1.iter().take(12) {
        let name = if *id == u32::MAX { "(dynamic shapes)" } else { game.cx.art.name(gfx::SpriteId(*id)) };
        let layers = if *id == u32::MAX { Vec::new() } else { renderer.layer_costs(*id) };
        let desc: Vec<String> = layers.iter().map(|(c, n, b)| format!("{c:.0}{}@{:.0}x{:.0}", if *n { "n" } else { "" }, b[2] - b[0], b[3] - b[1])).collect();
        println!("  {name:32} {:8.1} {count:4}  layers {}", px / 1e6, desc.join(" "));
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
    game.cx.settings.caps.mask(&mut display_cfg);
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
    if let Some(k) = o.render_scale {
        display_cfg.render_scale = k;
    }
    game.cx.settings.display = display_cfg;
    let mut platform = platform::sdl::Platform::new(app_info::NAME, display_cfg.width, display_cfg.height)?;
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
    fn app_version_is_the_crate_version() {
        let v = super::app_info::VERSION;
        assert_eq!(env!("CARGO_PKG_VERSION"), format!("{}.{}.{}", v[0], v[1], v[2]));
        assert_eq!(super::app_info::VERSION_TEXT, format!("{}.{}.{}.{}", v[0], v[1], v[2], v[3]));
    }

    #[test]
    fn window_icon_decodes() {
        let (w, h, rgba) = super::icon().expect("assets/icon/windows/wf_256.png");
        assert_eq!((w, h, rgba.len()), (256, 256, 256 * 256 * 4));
    }
}
