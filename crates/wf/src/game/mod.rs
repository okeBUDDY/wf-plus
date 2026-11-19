//! The game layer: the application loop (Application::Run), its task lists
//! and stages, and the services they share. Platform independent: the
//! frame goes out as a `DrawList`, sound through the mixer.

pub mod actor;
pub mod app;
pub mod assets;
pub mod background;
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
    /// g_fn_option's settings.
    pub options: fn_option::OptionState,
    /// What fnGame asked of its HUD.
    pub hud: hud::HudState,
    /// g_seq_flags (0x468fa0, shared with g_rating): what the game tells
    /// a .seq music.
    pub seq_flags: [u8; 8],
    /// Debug: called after every game frame's MoveAll (the state hash for
    /// replay desync checks, see `debug::state_hash`).
    pub frame_hook: Option<fn(&mut Cx)>,
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
            options: Default::default(),
            hud: Default::default(),
            seq_flags: [0; 8],
            frame_hook: None,
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
        let path = self.home.join("wf.sco");
        let Cx { scores, profile, .. } = self;
        scores.init(path, profile);
        fn_option::load_settings(self);
        records::init_records(self);
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
