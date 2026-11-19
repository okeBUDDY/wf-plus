//! The game layer: the application loop (Application::Run), its task lists
//! and stages, and the services they share. Platform independent: the
//! frame goes out as a `DrawList`, sound through the mixer.

pub mod actor;
pub mod app;
pub mod arena;
pub mod assets;
pub mod background;
pub mod debug;
pub mod enm;
pub mod fade;
pub mod fn_game;
pub mod fn_option;
pub mod fn_title;
pub mod hud;
pub mod game_state;
pub mod input;
pub mod math;
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
    /// The port's own settings (wf-plus.ini): the engine's display and
    /// input settings and the game's new ones; wf.ini stays the original's.
    pub port: wf_engine::config::Config,
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
            port: Default::default(),
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
            pack: crate::data::pack::Pack::from_bytes(vec![0; 8]).unwrap(),
            pats: Vec::new(),
            sounds: Vec::new(),
            bgm: None,
        };
        Cx::new(Art::load(), assets, 4)
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
