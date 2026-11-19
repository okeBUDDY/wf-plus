//! Warning Forever, ported to Rust.
//!
//! Layers, top to bottom:
//! * `game`: the port of the game (Application loop, tasks, stages, text,
//!   pads, sound system). Platform independent.
//! * seams: `gfx::DrawList` (what to draw), `platform::{RawInput, Event}`
//!   (what the player does), `sound::Mixer` (what to play).
//! * backends: `gfx::gpu` (wgpu, Slug rendering of the cooked art) and
//!   `platform::sdl` (SDL3 window, keyboard, gamepads, audio).

// Much of the game-facing API is ported ahead of the stages that use it.
#![allow(dead_code)]

mod art;
mod data;
mod game;
mod gfx;
mod log;
mod platform;
mod sound;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use game::assets::{Assets, CHANNELS};
use game::{App, Cx};
use platform::{Key, RawInput};

/// HLSync::Initialize (0x40c9e0): a 60 fps timer of (10000 / 60 + 5) / 10
/// = 17 ms, so the game really runs at 1000 / 17 = 58.8 frames a second.
const FRAME: Duration = Duration::from_millis(17);

struct Options {
    data: PathBuf,
    headless: bool,
    frames: u32,
    screenshot: Option<PathBuf>,
    size: (u32, u32),
    /// Headless input: keys held on given frames.
    script: Vec<(u32, Key)>,
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
        _ => return None,
    })
}

fn usage() -> ! {
    eprintln!(
        "usage: wf [--data <data.bin>] [--size WxH] [--frames N --screenshot out.png]\n          [--headless --script FRAME:KEY,...]\n\
         data.bin also comes from WF_DATA, else ./data.bin, else next to the executable."
    );
    std::process::exit(2);
}

fn options() -> Options {
    let mut o = Options {
        data: std::env::var_os("WF_DATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data.bin")),
        headless: false,
        frames: 60,
        screenshot: None,
        size: (1280, 960),
        script: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage());
        match a.as_str() {
            "--data" => o.data = value().into(),
            "--headless" => o.headless = true,
            "--frames" => o.frames = value().parse().unwrap_or_else(|_| usage()),
            "--screenshot" => o.screenshot = Some(value().into()),
            "--size" => {
                let v = value();
                let (w, h) = v.split_once('x').unwrap_or_else(|| usage());
                o.size = (w.parse().unwrap_or_else(|_| usage()), h.parse().unwrap_or_else(|_| usage()));
            }
            "--script" => {
                for item in value().split(',') {
                    let (f, k) = item.split_once(':').unwrap_or_else(|| usage());
                    o.script.push((f.parse().unwrap_or_else(|_| usage()), parse_key(k).unwrap_or_else(|| usage())));
                }
            }
            _ => usage(),
        }
    }
    if !o.data.exists() {
        if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
            let beside = exe_dir.join("data.bin");
            if beside.exists() {
                o.data = beside;
            }
        }
    }
    o
}

/// cGlobal::Initialize and WinMain's setup: everything the stages share.
fn startup(o: &Options) -> Result<(Cx, App), String> {
    let assets = Assets::load(&o.data)?;
    let mut cx = Cx::new(art::Art::load(), assets, CHANNELS);
    let mut app = App::new(Some(|| Box::new(game::viewer::Viewer::new())));
    if !app.start(&mut cx, Box::new(game::viewer::Viewer::new())) {
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

/// Runs `frames` frames without a window and saves the last one.
fn run_headless(o: &Options) -> Result<(), String> {
    let (mut cx, mut app) = startup(o)?;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let (_adapter, device, queue) = gfx::gpu::create_device(&instance, None)?;
    let mut renderer = gfx::gpu::Renderer::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, &cx.art.pack);
    for frame in 0..o.frames {
        let mut raw = RawInput::default();
        for &(f, k) in &o.script {
            if f == frame {
                raw.keys[k as usize] = true;
            }
        }
        cx.input.poll(&raw);
        app.frame(&mut cx);
        if !app.running() {
            break;
        }
    }
    if let Some(path) = &o.screenshot {
        renderer.time = cx.app.frame_count as f32 / 58.8;
        let rgba = renderer.render_to_rgba(&cx.draw, o.size.0, o.size.1);
        save_png(path, o.size.0, o.size.1, &rgba)?;
        println!("saved {}", path.display());
    }
    Ok(())
}

struct Display {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: gfx::gpu::Renderer,
}

impl Display {
    fn new(platform: &platform::sdl::Platform, art: &wf_art::Pack) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(platform.display_handle()?)));
        // SAFETY: the window outlives the surface: the Display is created
        // after the Platform in run_windowed and dropped before it.
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
        let present_mode = [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
            .into_iter()
            .find(|m| caps.present_modes.contains(m))
            .unwrap_or(wgpu::PresentMode::Fifo);
        let (w, h) = platform.drawable_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: w.max(1),
            height: h.max(1),
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: Default::default(),
        };
        surface.configure(&device, &config);
        let renderer = gfx::gpu::Renderer::new(device, queue, format, art);
        Ok(Display { surface, config, renderer })
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.config.width = w.max(1);
        self.config.height = h.max(1);
        self.surface.configure(self.renderer.device(), &self.config);
    }

    fn present(&mut self, list: &gfx::DrawList) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(self.renderer.device(), &self.config);
                return;
            }
            _ => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.renderer.render(list, &view, (self.config.width, self.config.height));
        self.renderer.queue().present(frame);
    }
}

fn run_windowed(o: &Options) -> Result<(), String> {
    let mut platform = platform::sdl::Platform::new("WARNING FOREVER", o.size.0, o.size.1)?;
    let (mut cx, mut app) = startup(o)?;
    let mut display = Display::new(&platform, &cx.art.pack)?;
    platform.start_audio(cx.sound.mixer());
    let mut next = Instant::now() + FRAME;
    let mut screenshot_taken = false;
    while app.running() {
        for event in platform.poll() {
            match event {
                platform::Event::Quit => app.quit(&mut cx),
                platform::Event::Resized(w, h) => display.resize(w, h),
                platform::Event::Focus(active) => {
                    if active {
                        cx.app.flags &= !game::AppFlags::INACTIVE;
                    } else {
                        cx.app.flags |= game::AppFlags::INACTIVE;
                    }
                }
            }
        }
        // The pads are polled while the window is active (InputTask).
        if cx.app.flags & game::AppFlags::INACTIVE == 0 {
            cx.input.poll(&platform.input());
        } else {
            cx.input.poll(&RawInput::default());
        }
        let drew = app.frame(&mut cx);
        // Wait for the timer tick, then present (HLSync's WaitBlt).
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        }
        next = (next + FRAME).max(Instant::now() - FRAME);
        if drew {
            display.renderer.time = cx.app.frame_count as f32 / 58.8;
            display.present(&cx.draw);
            if let (Some(path), false) = (&o.screenshot, screenshot_taken) {
                if cx.app.frame_count >= o.frames {
                    let (w, h) = (display.config.width, display.config.height);
                    let rgba = display.renderer.render_to_rgba(&cx.draw, w, h);
                    save_png(path, w, h, &rgba)?;
                    screenshot_taken = true;
                }
            }
        }
    }
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
