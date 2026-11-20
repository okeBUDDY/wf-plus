//! The game layer: the application loop (Application::Run), its task lists
//! and stages, and the services they share. Platform independent: the
//! frame goes out as a `DrawList`, sound through the mixer.

pub mod actor;
pub mod app;
pub mod arena;
pub mod settings;
pub mod assets;
pub mod background;
pub mod debug;
pub mod enm;
pub mod fade;
pub mod fn_game;
pub mod fn_option;
pub mod fn_title;
pub mod guide;
pub mod hud;
pub mod game_state;
pub mod input;
pub mod math;
pub mod pointer;
pub mod profile;
pub mod records;
pub mod replay;
pub mod score_table;
pub mod sounds;
pub mod task;
pub mod texels;
pub mod text;
pub mod time;
pub mod viewer;
#[cfg(all(test, target_arch = "x86_64"))]
mod x87_tests;

use crate::art::Art;
use crate::gfx::DrawList;
use crate::sound::SoundSystem;
pub use app::{App, AppFlags};
pub use assets::Assets;
use input::Input;
use task::TaskList;
use text::Fonts;

/// Everything a task or stage can reach (the original's globals).
pub struct Cx {
    pub draw: DrawList,
    pub art: Art,
    pub assets: Assets,
    pub fonts: Fonts,
    pub input: Input,
    pub sound: SoundSystem,
    pub tasks: TaskList,
    pub overlays: TaskList,
    pub app: app::AppState,
    /// The screen's flash and fade (g_global.screen_fade), drawn by the
    /// `fade::FadeTask` overlay.
    pub fade: fade::ScreenFade,
    /// The patterns of the sprite sheets (the HLSPat files).
    pub pats: actor::Pats,
    /// g_actor_system.
    pub actors: actor::ActorSystem,
    /// The static data of the actor controls.
    pub ctl: actor::controls::ControlData,
    /// g_enm_param: the boss generator.
    pub enm: enm::EnmParam,
    /// g_game.
    pub game: game_state::Game,
    /// g_global's game fields.
    pub global: game_state::Global,
    /// g_fn_game's fields.
    pub fn_game: fn_game::FnGameState,
    /// g_replay.
    pub replay: replay::Replay,
    /// The C runtime's rand() state.
    pub rng: math::Rand,
    /// Texts drawn by actors (`actor::Obj::Text`).
    pub obj_texts: Vec<text::ObjText>,
    /// Where the settings, the scores and the replays live.
    pub home: std::path::PathBuf,
    /// The settings file (Application::profile, wf.ini).
    pub profile: profile::Profile,
    /// g_score_table.
    pub scores: score_table::ScoreTable,
    /// The other arena mode's table (wf-hd.sco or wf.sco), swapped
    /// with `scores` by `select_scores`.
    pub scores_alt: score_table::ScoreTable,
    /// g_fn_option's settings.
    pub options: fn_option::OptionState,
    /// What fnGame asked of its HUD.
    pub hud: hud::HudState,
    /// g_seq_flags (0x468fa0, shared with g_rating): what the game tells
    /// a .seq music.
    pub seq_flags: [u8; 8],
    /// Debug: called after every game frame's MoveAll (the state hash for
    /// replay desync checks, see `debug::start_hash_log`).
    pub frame_hook: Option<fn(&mut Cx)>,
    /// The arena the game runs in (Classic outside a game; locked when a
    /// game starts, or read from the replay).
    pub arena: arena::Arena,
    /// The arena a game started now would get: the mode the player picked
    /// and the width the window gives (not game state until a game locks
    /// it).
    pub arena_choice: arena::Mode,
    pub window_arena_width: f32,
    /// The window's size in pixels (display only).
    pub window_size: (u32, u32),
    /// The port's own settings (wf-plus.ini): the engine's display and
    /// input settings and the game's new ones; wf.ini stays the original's.
    pub port: wf_engine::config::Config,
    /// The port's settings (from `port`): display and input tuning.
    pub settings: settings::Settings,
    /// The menus' pointer and tab input.
    pub ui: settings::UiInput,
    /// The input guide this frame's screen asks for (`guide`; display
    /// only, cleared each frame).
    pub guide: Option<guide::Guide>,
}

impl Cx {
    /// A context around the loaded art and assets, with the game state at
    /// its start-up values (cGlobal::Initialize).
    pub fn new(art: Art, assets: Assets, channels: usize) -> Self {
        let fonts = Fonts::load(&art);
        let pats = actor::Pats::load(&assets, &art);
        let plan = assets.pack.get("plan.dat").and_then(enm::EnmPlan::load).unwrap_or_default();
        let mut cx = Cx {
            draw: DrawList::new(),
            art,
            assets,
            fonts,
            input: Input::default(),
            sound: SoundSystem::new(channels),
            tasks: Default::default(),
            overlays: Default::default(),
            app: Default::default(),
            fade: Default::default(),
            pats,
            actors: actor::ActorSystem::new(),
            ctl: Default::default(),
            enm: enm::EnmParam::new(plan),
            game: Default::default(),
            global: Default::default(),
            fn_game: Default::default(),
            replay: Default::default(),
            rng: Default::default(),
            obj_texts: Vec::new(),
            home: std::path::PathBuf::from("."),
            profile: Default::default(),
            scores: Default::default(),
            scores_alt: Default::default(),
            options: Default::default(),
            hud: Default::default(),
            seq_flags: [0; 8],
            frame_hook: None,
            arena: arena::Arena::CLASSIC,
            arena_choice: arena::Mode::Classic,
            window_arena_width: 640.0,
            window_size: (640, 480),
            port: Default::default(),
            settings: Default::default(),
            ui: Default::default(),
            guide: None,
        };
        actor::controls::register_all(&mut cx);
        let Cx { enm, rng, .. } = &mut cx;
        enm.reset(rng);
        task::add(&mut cx, task::ListId::Overlays, Box::new(fade::FadeTask), 1, task::Place::Back);
        cx
    }

    /// The file side of cGlobal::Initialize: the settings in `home`/wf.ini,
    /// the scores in wf.sco and the replays in rec/.
    pub fn open_home(&mut self, home: std::path::PathBuf) {
        self.home = home;
        let _ = std::fs::create_dir_all(&self.home);
        self.profile = profile::Profile::open(self.home.join("wf.ini"));
        self.port = wf_engine::config::Config::load(&self.home.join("wf-plus.ini"));
        arena::load_choice(self);
        self.settings = settings::Settings::load(&self.port);
        let path = self.home.join("wf.sco");
        let Cx { scores, profile, .. } = self;
        scores.init(path, profile);
        self.scores_alt.init_hd(self.home.join("wf-hd.sco"));
        fn_option::load_settings(self);
        self.select_scores(self.arena_choice);
        records::init_records(self);
    }

    /// Locks `arena` for the game: the bounds, the culling and the hit
    /// image follow it, and the frames are drawn in it.
    pub fn set_arena(&mut self, arena: arena::Arena) {
        self.arena = arena;
        // Only the sides: bounds_.top moves in the game (Control32::Dash),
        // and the original keeps it from game to game.
        let [left, _, right, _] = arena.boss_bounds();
        self.ctl.boss_bounds.left = left;
        self.ctl.boss_bounds.right = right;
        self.actors.set_arena(&arena);
        self.draw.arena = [arena.width, arena.height];
    }

    /// Makes `scores` the table of arena mode `mode`.
    pub fn select_scores(&mut self, mode: arena::Mode) {
        if self.scores.is_hd() != (mode == arena::Mode::Hd) {
            std::mem::swap(&mut self.scores, &mut self.scores_alt);
        }
    }

    /// The view the HUD keeps to, as (left, width) in arena units: the
    /// arena in HD; in Classic the window's whole width around the
    /// 640-wide arena (display only: the game never reads it). Either way
    /// the part of the camera's view (`gfx::Layout::visible` for the frame's
    /// arena) that holds the arena's height.
    pub fn hud_view(&self) -> (f32, f32) {
        if self.arena.is_classic() {
            let (w, h) = self.window_size;
            let w = (480.0 * w.max(1) as f32 / h.max(1) as f32).max(arena::W);
            ((arena::W - w) * 0.5, w)
        } else {
            (0.0, self.arena.width)
        }
    }

    /// The window as a rect (x, y, width, height) in the units of the
    /// 640x480 arena it is centred on: 480 high and as wide as the window's
    /// aspect, or 640 wide and taller for a window narrower than 4:3. The
    /// menus lay out to it (display only).
    ///
    /// With an input guide the menus' screen is `guide::STRIP` taller
    /// (below 480); this is the part above the strip.
    pub fn screen_view(&self) -> [f32; 4] {
        let [x, y, w, h] = self.window_view();
        [x, y, w, h - guide::menu_strip(self)]
    }

    /// The whole window in the same units, guide strip included.
    pub fn window_view(&self) -> [f32; 4] {
        let aspect = self.window_arena_width.max(1.0) / 480.0;
        let bh = 480.0 + guide::menu_strip(self);
        let (w, h) = if bh * aspect >= arena::W { (bh * aspect, bh) } else { (arena::W, arena::W / aspect) };
        [(arena::W - w) * 0.5, (bh - h) * 0.5, w, h]
    }

    /// The arena a game started now gets.
    pub fn chosen_arena(&self) -> arena::Arena {
        match self.arena_choice {
            arena::Mode::Classic => arena::Arena::CLASSIC,
            arena::Mode::Hd => arena::Arena::hd(self.window_arena_width),
        }
    }

    /// The screen fade (g_global.screen_fade).
    pub fn fade(&mut self) -> &mut fade::ScreenFade {
        &mut self.fade
    }

    /// Starts the music (`channel(kChannelBgm)->Assign(&g_bgm, kLoop)`).
    pub fn play_bgm(&mut self) {
        if let Some(bgm) = &self.assets.bgm {
            self.sound.play_music(assets::CHANNEL_BGM, bgm, true, 1.0);
        }
    }

    /// Plays sound `id` on its own channel (SndSys::Play).
    pub fn play_sound(&mut self, id: usize) {
        self.play_sound_on(id, id);
    }

    /// Plays sound `id` on `channel`.
    pub fn play_sound_on(&mut self, channel: usize, id: usize) {
        if let Some(Some(buf)) = self.assets.sounds.get(id) {
            self.sound.play(channel, buf, false, 1.0);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A context without data.bin (no sounds, no patterns).
    pub fn cx() -> Cx {
        let assets = Assets {
            pack: wf_engine::assets::Registry::new(),
            pats: Vec::new(),
            sounds: Vec::new(),
            bgm: None,
        };
        Cx::new(Art::load(), assets, 4)
    }

    /// The renderer's start-up cost estimate goes through the whole pack
    /// in well under a second: it once read the noise lattices as band
    /// counts in the billions (75 s at start-up, never done on a phone).
    #[test]
    fn startup_layer_costs_are_quick_and_sane() {
        let art = Art::load();
        let t = std::time::Instant::now();
        let costs = crate::gfx::gpu::layer_curve_estimates(&art.pack);
        let took = t.elapsed();
        assert!(took.as_secs_f32() < 1.0, "layer cost estimate took {took:?}");
        assert_eq!(costs.len(), art.pack.layers.len());
        let max = costs.iter().copied().fold(0.0f32, f32::max);
        assert!(costs.iter().all(|c| c.is_finite() && *c >= 0.0) && max < 10_000.0, "max {max} curves per pixel");
        assert!(max > 1.0, "the shapes have curves");
    }

    #[test]
    fn art_pack_has_every_sheet_and_font() {
        let art = Art::load();
        for name in ["enm0_001", "add0_025", "sys0_008", "etc2_logo", "etc2_001_character", "font/kana/000", "font/title/026"] {
            assert!(art.find(name).is_some(), "{name} is cooked");
        }
    }

    #[test]
    fn text_draws_one_glyph_per_character() {
        let mut cx = cx();
        text::print(&mut cx, |f| &f.text, 0.0, 0.0, "AB C@@2D", crate::gfx::Argb::WHITE);
        assert_eq!(cx.draw.draws.len(), 4);
        let kana = text::sjis("ワ★");
        cx.draw.reset();
        let f = &cx.fonts.kana;
        f.print(&mut cx.draw, 0.0, 0.0, &kana, None, crate::gfx::Argb::WHITE, -1, crate::gfx::Blend::Normal);
        assert_eq!(cx.draw.draws.len(), 2);
    }
}

/// Slug coverage checks on the GPU (skipped when no adapter is found).
#[cfg(test)]
mod coverage_tests {
    use crate::gfx::{Argb, Blend, DrawList, Transform};

    fn renderer(art: &crate::art::Art) -> Option<crate::gfx::gpu::Renderer> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let (_, device, queue) = crate::gfx::gpu::create_device(&instance, None).ok()?;
        let mut r = crate::gfx::gpu::Renderer::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, &art.pack);
        art.register_line_shapes(&mut r);
        Some(r)
    }

    /// Pixels inside `rect` (x0, y0, x1, y1) of an RGBA image that are not
    /// black.
    fn holes(rgba: &[u8], width: u32, rect: [u32; 4]) -> usize {
        let mut n = 0;
        for y in rect[1]..rect[3] {
            for x in rect[0]..rect[2] {
                let p = &rgba[((y * width + x) * 4) as usize..][..3];
                if p.iter().any(|&c| c != 0) {
                    n += 1;
                }
            }
        }
        n
    }

    /// An opaque black fill over white must leave no pixel inside it
    /// uncovered, at any scale. The fill is the screen rectangle: two
    /// triangles whose shared diagonal is stored once each way and must
    /// cancel exactly.
    #[test]
    fn filled_quad_has_no_holes() {
        let art = crate::art::Art::load();
        let Some(mut r) = renderer(&art) else { return };
        let mut failed = Vec::new();
        for (w, h) in [(1280, 720), (640, 480), (2000, 1253), (1000, 1000)] {
            let mut list = DrawList::new();
            list.clear = Argb(0xffff_ffff);
            list.sprite(art.shapes.screen, &Transform::IDENTITY, Argb(0xff00_0000), Blend::Normal);
            let layout = crate::gfx::Layout::new(list.arena, (w, h));
            let [ax, ay, aw, ah] = layout.arena_px;
            let rgba = r.render_to_rgba(&list, w, h);
            // One pixel in from the arena's edges (they are antialiased).
            let n = holes(&rgba, w, [ax + 1, ay + 1, ax + aw - 1, ay + ah - 1]);
            if n != 0 {
                failed.push(format!("{n} at {w}x{h}"));
            }
        }
        assert!(failed.is_empty(), "uncovered pixels: {}", failed.join(", "));
    }

    /// The sprite cache draws every cached sprite as Slug does: each one at
    /// several scales (between and on the cache's buckets), turns, a
    /// stretch and sub-pixel offsets, tinted and additive, cache on against
    /// off. Only antialiasing may differ.
    #[test]
    fn sprite_cache_matches_slug() {
        let art = crate::art::Art::load();
        let Some(mut r) = renderer(&art) else { return };
        let (w, h) = (1280u32, 720u32);
        let mut worst = (0u8, 0.0f64, String::new());
        let mut worst_mean = 0.0f64;
        let mut report = Vec::new();
        for id in 0..art.count() as u32 {
            if !r.sprite_cacheable(id) {
                continue;
            }
            let b = art.pack.sprites[id as usize].bounds;
            let size = (b[2] - b[0]).max(b[3] - b[1]).max(1.0);
            let mut list = DrawList::new();
            list.clear = Argb(0xff20_2020);
            let mut i = 0;
            for &(sx, sy) in &[(0.45f32, 0.45f32), (0.8, 0.8), (1.0, 1.0), (1.13, 1.13), (1.7, 0.7), (2.3, 2.3)] {
                for &turn in &[0.0f32, 0.37, 2.1] {
                    let s = 70.0 / size;
                    let mut t = Transform::translation(-(b[0] + b[2]) * 0.5, -(b[1] + b[3]) * 0.5);
                    t.scale(sx * s, sy * s);
                    t.rotate_z(turn);
                    let (cx, cy) = ((i % 8) as f32 * 80.0 + 40.0 + 0.3 * i as f32 % 1.0, (i / 8) as f32 * 160.0 + 80.0 + 0.17 * i as f32 % 1.0);
                    t.translate(cx, cy);
                    let (color, blend) = if i % 3 == 2 { (Argb(0x80ff_c040), Blend::Additive) } else { (Argb(0xffff_ffff), Blend::Normal) };
                    if i % 4 == 3 {
                        // A sheet's texel region (the HUD's banners and labels).
                        let clip = [b[0] + (b[2] - b[0]) * 0.2, b[1] + (b[3] - b[1]) * 0.3, b[2] - (b[2] - b[0]) * 0.25, b[3]];
                        list.sprite_clipped(crate::gfx::SpriteId(id), &t, clip, color, blend);
                    } else {
                        list.sprite(crate::gfx::SpriteId(id), &t, color, blend);
                    }
                    i += 1;
                }
            }
            r.set_sprite_cache(false);
            let slug = r.render_to_rgba(&list, w, h);
            r.set_sprite_cache(true);
            for _ in 0..64 {
                let _ = r.render_to_rgba(&list, w, h);
                if r.stats.cache.rasterized == 0 {
                    break;
                }
            }
            let cached = r.render_to_rgba(&list, w, h);
            assert_eq!(r.stats.cache.deferred, 0);
            let (mut max, mut sum) = (0u8, 0u64);
            for (a, b) in slug.iter().zip(&cached) {
                let d = a.abs_diff(*b);
                max = max.max(d);
                sum += d as u64;
            }
            let mean = sum as f64 / slug.len() as f64 * 4.0 / 3.0;
            let name = art.name(crate::gfx::SpriteId(id)).to_string();
            if let Some(dir) = std::env::var_os("WF_CACHE_DUMP") {
                let file = name.replace('/', "_");
                std::fs::write(std::path::Path::new(&dir).join(format!("{file}_slug.rgba")), &slug).unwrap();
                std::fs::write(std::path::Path::new(&dir).join(format!("{file}_cached.rgba")), &cached).unwrap();
            }
            report.push(format!("{name} max {max} mean {mean:.3}"));
            worst_mean = worst_mean.max(mean);
            if max > worst.0 {
                worst = (max, mean, name);
            }
        }
        println!("{}", report.join("\n"));
        assert!(!report.is_empty(), "no sprite is cached");
        // Antialiasing differences only: a shifted or softened sprite moves
        // the mean far more (a one-tap 1x atlas, too soft on thin lines,
        // measured up to 0.94 on these sprites; four taps at 2x stay under
        // 0.5).
        assert!(worst_mean < 0.5, "a cached sprite differs from Slug: {}", report.join(", "));
        println!("worst: {} max {} mean {:.3}", worst.2, worst.0, worst.1);
    }

    /// The analytic line quads draw the line shapes as Slug does: the
    /// cooked ones (the hexagon turned, scaled and faded as on the warning
    /// screen, the cursors, the grid's lines) and runtime-built strips and
    /// lists (the aim cone's sides, a polyline with sharp turns and one
    /// whose segments cross), normal and additive, lines on against off.
    #[test]
    fn analytic_lines_match_slug() {
        let art = crate::art::Art::load();
        let Some(mut r) = renderer(&art) else { return };
        let (w, h) = (1280u32, 720u32);
        let mut list = DrawList::new();
        list.clear = Argb(0xff20_2020);
        let sh = &art.shapes;
        let mut i = 0;
        for &(id, scale) in &[(sh.hexagon, 20.0f32), (sh.hexagon, 7.3), (sh.menu_cursor, 1.5), (sh.title_line, 1.0), (sh.name_cursor, 2.0), (sh.grid_line, 1.0)] {
            for &turn in &[0.0f32, 0.41, 1.9] {
                let mut t = Transform::IDENTITY;
                t.scale(scale, scale * if i % 2 == 1 { 0.7 } else { 1.0 });
                t.rotate_z(turn);
                t.translate((i % 6) as f32 * 100.0 + 50.3, (i / 6) as f32 * 100.0 + 50.6);
                let (color, blend) = if i % 3 == 2 { (Argb(0x80ff_c040), Blend::Additive) } else { (Argb(0xc0ff_ffff), Blend::Normal) };
                list.sprite(id, &t, color, blend);
                i += 1;
            }
        }
        let cone = [[0.0, 0.0], [-60.0, -150.0], [60.0, -150.0], [0.0, 0.0]];
        let zigzag = [[0.0, 0.0], [60.0, 8.0], [5.0, 20.0], [70.0, 30.0], [70.0, 31.0], [10.0, 90.0]];
        let crossing = [[0.0, 0.0], [80.0, 80.0], [80.0, 0.0], [0.0, 80.0]];
        let pairs = [[0.0, 0.0], [90.0, 10.0], [90.0, 10.0], [100.0, 60.0], [20.0, 70.0], [60.0, 30.0]];
        let mut t = Transform::translation(560.4, 470.2);
        list.shape(crate::gfx::Primitive::LineStrip, &t, &cone, Argb(0x60ff_4040), Blend::Additive);
        t = Transform::translation(20.25, 330.5);
        list.shape(crate::gfx::Primitive::LineStrip, &t, &zigzag, Argb(0xffff_ffff), Blend::Normal);
        t.rotate_z(0.3);
        t.translate(150.0, 0.0);
        list.shape(crate::gfx::Primitive::LineStrip, &t, &crossing, Argb(0xa040_ff40), Blend::Normal);
        t.translate(150.0, 0.0);
        list.shape(crate::gfx::Primitive::LineList, &t, &pairs, Argb(0xffff_ffff), Blend::Additive);
        r.collect = true;
        r.lines_enabled = false;
        let slug = r.render_to_rgba(&list, w, h);
        r.lines_enabled = true;
        let lines = r.render_to_rgba(&list, w, h);
        assert!(r.stats.line_px > 0.0, "no line quads drawn");
        let (mut max, mut sum, mut far) = (0u8, 0u64, 0usize);
        for (a, b) in slug.iter().zip(&lines) {
            let d = a.abs_diff(*b);
            max = max.max(d);
            sum += d as u64;
            far += (d > 8) as usize;
        }
        let mean = sum as f64 / slug.len() as f64 * 4.0 / 3.0;
        if let Some(dir) = std::env::var_os("WF_CACHE_DUMP") {
            std::fs::write(std::path::Path::new(&dir).join("lines_slug.rgba"), &slug).unwrap();
            std::fs::write(std::path::Path::new(&dir).join("lines_quads.rgba"), &lines).unwrap();
        }
        println!("lines vs Slug: max {max} mean {mean:.4}, {far} channels off by more than 8");
        assert!(far < 64 && mean < 0.02, "the line quads differ from Slug: max {max} mean {mean:.4}, {far} channels > 8");
    }

    /// Not a check: renders every sprite and glyph, white-tinted over black,
    /// into 1280x720 pages written as raw RGBA to `$WF_COVERAGE_DUMP` (for
    /// comparing coverage across shader changes).
    #[test]
    #[ignore]
    fn contact_sheet() {
        let Some(dir) = std::env::var_os("WF_COVERAGE_DUMP") else { return };
        let art = crate::art::Art::load();
        let Some(mut r) = renderer(&art) else { return };
        const COLS: usize = 16;
        const ROWS: usize = 12;
        const CELL: f32 = 40.0;
        let ids: Vec<u32> = (0..art.count() as u32).collect();
        for (page, chunk) in ids.chunks(COLS * ROWS).enumerate() {
            let mut list = DrawList::new();
            list.clear = Argb(0xff00_0000);
            for (i, &id) in chunk.iter().enumerate() {
                let b = art.pack.sprites[id as usize].bounds;
                let s = ((CELL - 4.0) / (b[2] - b[0]).max(b[3] - b[1]).max(1.0)).min(4.0);
                let mut t = Transform::translation(-(b[0] + b[2]) * 0.5, -(b[1] + b[3]) * 0.5);
                t.scale(s, s);
                t.translate((i % COLS) as f32 * CELL + CELL * 0.5, (i / COLS) as f32 * CELL + CELL * 0.5);
                list.sprite(crate::gfx::SpriteId(id), &t, Argb(0xffff_ffff), Blend::Normal);
            }
            let rgba = r.render_to_rgba(&list, 1280, 720);
            std::fs::write(std::path::Path::new(&dir).join(format!("sheet_{page}.rgba")), &rgba).unwrap();
        }
    }
}
