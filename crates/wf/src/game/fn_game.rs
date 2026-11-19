//! fnGame (fn_game.cpp): the stage that runs a game. The state the actors
//! read (the player's keys, the stage state) lives in `Cx::fn_game`; the
//! stage object itself only owns its tasks.

/// FnGame's fields (g_fn_game, 0x467c98).
#[derive(Default, Clone, Debug)]
pub struct FnGameState {
    pub flags: u32,
    /// 0 while recording, else playing back.
    pub mode: i32,
    pub bgm_mode: i32,
    pub exit_frames: i16,
    pub exit_time: i16,
    pub state: u16,
    pub state_time: u16,
    pub stage_score: u32,
    /// Held this frame.
    pub keys: u16,
    /// Pressed this frame.
    pub pressed: u16,
}

impl FnGameState {
    pub const PLAYBACK: u32 = 0x1;
    pub const REPLAY_LIST: u32 = 0x2;
    pub const MUSIC: u32 = 0x4;

    pub const START: u16 = 10;
    pub const STAGE: u16 = 20;
    pub const BATTLE: u16 = 30;
    pub const CLEARED: u16 = 40;
    pub const GAME_OVER: u16 = 50;

    /// SetState (0x41be80).
    pub fn set_state(&mut self, state: u16) {
        self.state = state;
        self.state_time = 0;
    }
}

use super::Cx;
use super::actor;
use super::app::Stage;
use super::assets::CHANNEL_BGM;
use super::background::BackgroundGrid;
use super::enm::battle;
use super::game_state::{self, FLAG_PAUSE, GAME_OVER, RUNNING};
use super::hud::{self, GameHud};
use super::input;
use super::records;
use super::replay::{self, Mode};
use super::task::{self, ListId, Place, TaskId};

/// cGlobal::ClearStage (0x4011a0).
pub fn clear_stage(cx: &mut Cx) {
    actor::destroy_all(cx);
    cx.fade.set_color(0, 0, 0, 0);
}

/// cGlobal::ResetSound (0x4011f0).
pub fn reset_sound(cx: &mut Cx) {
    cx.sound.stop_group(None);
    cx.sound.mute_group(false, None);
    cx.sound.set_group_volume(cx.global.effect_volume, Some(0));
    cx.sound.set_group_volume(cx.global.music_volume, Some(1));
}

/// cGlobal::ResetGame (0x401240).
pub fn reset_game(cx: &mut Cx) {
    cx.game.boss = None;
    cx.game.player = None;
    cx.global.stage = 0;
    cx.global.entry.score = 0;
    cx.global.entry.deaths = 0;
    if cx.fn_game.mode == 0 {
        cx.scores.clear_rank();
    }
    let Cx { enm, rng, .. } = cx;
    enm.reset(rng);
}

/// ClearSeqFlags (0x40ea20).
pub fn clear_seq_flags(cx: &mut Cx) {
    cx.seq_flags = [0; 8];
}

impl FnGameState {
    /// SetMode (0x41be30): record (0) or play back `path` (1: the replay
    /// list, 2: the title's demo).
    pub fn set_mode(cx: &mut Cx, mode: i32, path: Option<&str>) {
        cx.fn_game.mode = mode;
        match mode {
            0 => {
                let p = cx.home_path(records::TEMP);
                cx.replay.open(Mode::Record, p);
            }
            1 | 2 => {
                let p = cx.home_path(path.unwrap_or(""));
                cx.replay.open(Mode::Play, p);
            }
            _ => {}
        }
    }
}

/// The game stage (FnGame).
#[derive(Default)]
pub struct FnGame {
    background: Option<TaskId>,
    hud: Option<TaskId>,
}

impl FnGame {
    pub fn new() -> Self {
        Self::default()
    }

    /// Exit (0x41c140): fades out in `fade` frames and ends the stage
    /// `wait` frames after that.
    fn exit(cx: &mut Cx, fade: i32, wait: i32) {
        let g = &mut cx.fn_game;
        if g.exit_frames == 0 {
            g.exit_frames = (fade + wait) as i16;
            g.exit_time = 0;
            cx.fade.fade_out(fade as i16);
        }
    }

    /// SetSeqFlags (0x41c170).
    fn set_seq_flags(cx: &mut Cx) {
        clear_seq_flags(cx);
        cx.seq_flags[0] = 1;
        cx.seq_flags[4] = cx.fn_game.bgm_mode as u8;
    }

    fn title() -> Box<dyn Stage> {
        Box::new(super::fn_title::FnTitle::new())
    }
}

impl Stage for FnGame {
    /// Initialize (0x41bf50).
    fn initialize(&mut self, cx: &mut Cx) -> bool {
        clear_stage(cx);
        reset_game(cx);
        let rules = cx.global.rules;
        cx.game.start(&rules);
        actor::player::reset_weapon(cx);
        let g = &mut cx.fn_game;
        g.stage_score = 0;
        g.exit_time = 0;
        g.exit_frames = 0;
        g.flags = 0;
        self.background = task::add(cx, ListId::Tasks, Box::new(BackgroundGrid::new(0xff00_4030, 8.0)), 2, Place::Back);
        self.hud = task::add(cx, ListId::Overlays, Box::new(GameHud::new()), 2, Place::Front);
        cx.fn_game.set_state(FnGameState::START);
        let pause = cx.global.flags & FLAG_PAUSE != 0;
        match cx.fn_game.mode {
            0 => {
                if cx.assets.bgm.is_some() {
                    cx.fn_game.flags |= FnGameState::MUSIC;
                }
            }
            1 => cx.fn_game.flags |= FnGameState::PLAYBACK | FnGameState::REPLAY_LIST,
            2 => {
                cx.sound.mute_group(true, Some(1));
                cx.fn_game.flags |= FnGameState::PLAYBACK;
                return true;
            }
            _ => {}
        }
        if pause {
            cx.input.time_stop_mask = input::PAUSE;
        }
        true
    }

    /// Finalize (0x41c0e0).
    fn finalize(&mut self, cx: &mut Cx) {
        cx.replay.close();
        cx.input.time_stop_mask = 0;
        actor::destroy_all(cx);
        reset_sound(cx);
        if let Some(id) = self.background.take() {
            task::delete(cx, ListId::Tasks, id);
        }
        if let Some(id) = self.hud.take() {
            task::delete(cx, ListId::Overlays, id);
        }
    }

    /// Move (0x41c190).
    fn update(&mut self, cx: &mut Cx) {
        if cx.fn_game.flags & FnGameState::PLAYBACK != 0 {
            let pressed = cx.input.pad(0).pressed;
            if pressed & (input::SHOT | input::SLOW | input::WEAPON_SELECT) != 0 {
                if pressed & input::SLOW != 0 {
                    cx.fn_game.flags &= !FnGameState::REPLAY_LIST;
                }
                Self::exit(cx, 30, 0);
            }
        }
        cx.fn_game.state_time = cx.fn_game.state_time.wrapping_add(1);
        let mut in_battle = false;
        let mut state = cx.fn_game.state;
        // The cases fall through into the next state as the original's.
        loop {
            match state {
                FnGameState::BATTLE => {
                    cx.actors.flags |= 1;
                    cx.game.flags |= RUNNING;
                    let player = cx.game.player;
                    cx.game.boss = battle::start_battle(cx, player);
                    cx.fn_game.state += 1;
                    state += 1;
                    continue;
                }
                s if s == FnGameState::BATTLE + 1 => {
                    let (p, b) = (cx.game.player, cx.game.boss);
                    battle::run_battle(cx, p, b);
                    in_battle = true;
                }
                FnGameState::STAGE => {
                    cx.global.stage += 1;
                    if cx.fn_game.flags & FnGameState::MUSIC != 0 {
                        records::rate_game(cx);
                        match cx.fn_game.bgm_mode {
                            1 => {
                                if cx.global.stage > 1 {
                                    cx.sound.fade(CHANNEL_BGM, 0.0, 1.0, 3.0, false);
                                }
                            }
                            2 => cx.play_bgm(),
                            _ => {}
                        }
                    }
                    cx.hud.request_mode(hud::MODE_GAME_OVER);
                    let player = cx.game.player;
                    replay::start(cx, player);
                    cx.fn_game.state += 1;
                    state += 1;
                    continue;
                }
                s if s == FnGameState::STAGE + 1 => {
                    if !cx.hud.busy() {
                        cx.fn_game.set_state(FnGameState::BATTLE);
                    }
                }
                FnGameState::CLEARED => {
                    crate::log::info(&format!(
                        "stage {} cleared: score {} timer {} bonus {} frames {} rand {:#x}",
                        cx.global.stage, cx.global.entry.score, cx.game.timer, cx.game.bonus, cx.game.frames, cx.rng.seed
                    ));
                    cx.game.flags &= !RUNNING;
                    cx.game.on_boss_destroyed();
                    cx.replay.close();
                    if cx.fn_game.flags & FnGameState::MUSIC != 0 {
                        match cx.fn_game.bgm_mode {
                            1 => cx.sound.fade(CHANNEL_BGM, 1.0, 0.0, 5.0, false),
                            2 => cx.sound.fade(CHANNEL_BGM, 1.0, 0.0, 5.0, true),
                            _ => {}
                        }
                    }
                    if cx.fn_game.flags & FnGameState::PLAYBACK == 0 {
                        records::save_replay(cx, false);
                        cx.hud.request_mode(hud::MODE_RESULT);
                    }
                    cx.actors.flags &= !1;
                    cx.fn_game.state += 1;
                    state += 1;
                    continue;
                }
                s if s == FnGameState::CLEARED + 1 => {
                    if !cx.hud.busy() && cx.game.boss.is_none() {
                        cx.fn_game.state += 1;
                    }
                }
                s if s == FnGameState::CLEARED + 2 => {
                    if cx.fn_game.flags & FnGameState::PLAYBACK != 0 {
                        Self::exit(cx, 60, 60);
                    } else {
                        let gained = cx.global.entry.score.wrapping_sub(cx.fn_game.stage_score);
                        let stage = cx.global.stage;
                        let Cx { enm, rng, .. } = cx;
                        enm.plan(gained, stage, rng);
                        cx.fn_game.stage_score = cx.global.entry.score;
                        cx.fn_game.set_state(FnGameState::STAGE);
                    }
                }
                FnGameState::GAME_OVER => {
                    crate::log::info(&format!("game over at stage {}: score {} frames {}", cx.global.stage, cx.global.entry.score, cx.game.frames));
                    cx.game.flags &= !RUNNING;
                    cx.replay.close();
                    if let Some(p) = cx.game.player {
                        actor::destroy(cx, p, true, true);
                    }
                    if cx.fn_game.flags & FnGameState::PLAYBACK == 0 {
                        records::save_replay(cx, true);
                        cx.global.entry.stage = cx.global.stage as i16;
                        cx.global.entry.time = cx.game.frames as u32;
                        cx.hud.request_mode(hud::MODE_NAME_ENTRY);
                    }
                    cx.fn_game.state += 1;
                    state += 1;
                    continue;
                }
                s if s == FnGameState::GAME_OVER + 1 => {
                    if !cx.hud.busy() && cx.fn_game.exit_frames == 0 {
                        Self::exit(cx, 60, 60);
                    }
                    let (p, b) = (cx.game.player, cx.game.boss);
                    battle::run_battle(cx, p, b);
                }
                FnGameState::START => {
                    if !replay::load_header(cx) {
                        cx.app.finish(Some(Self::title()));
                        break;
                    }
                    cx.actors.flags &= !1;
                    game_state::spawn_player(cx);
                    let player = cx.game.player;
                    match player {
                        Some(p) if replay::load_player(cx, p) => {}
                        _ => {
                            cx.app.finish(Some(Self::title()));
                            break;
                        }
                    }
                    if cx.fn_game.flags & FnGameState::MUSIC != 0 {
                        Self::set_seq_flags(cx);
                        if cx.fn_game.bgm_mode != 2 {
                            cx.play_bgm();
                        }
                    }
                    cx.fade.fade_in(60);
                    cx.fn_game.state += 1;
                    state += 1;
                    continue;
                }
                s if s == FnGameState::START + 1 => {
                    if cx.fn_game.state_time >= 120 {
                        cx.fn_game.set_state(FnGameState::STAGE);
                    }
                }
                _ => {}
            }
            break;
        }
        cx.game.tick();
        if in_battle && cx.game.flags & GAME_OVER != 0 {
            cx.fn_game.set_state(FnGameState::GAME_OVER);
        }
        if cx.fn_game.exit_frames != 0 {
            cx.fn_game.exit_time += 1;
            if cx.fn_game.exit_time >= cx.fn_game.exit_frames {
                let next = if cx.fn_game.flags & FnGameState::REPLAY_LIST != 0 { records::next_replay(cx) } else { None };
                if let Some(path) = next {
                    cx.fn_game.mode = 1;
                    let p = cx.home_path(&path);
                    cx.replay.open(Mode::Play, p);
                    cx.app.finish(Some(Box::new(FnGame::new())));
                } else {
                    cx.app.finish(Some(Self::title()));
                }
            }
            let volume = 1.0 - cx.fn_game.exit_time as f32 / cx.fn_game.exit_frames as f32;
            cx.sound.set_group_volume(cx.global.effect_volume * volume, Some(0));
            cx.sound.set_group_volume(cx.global.music_volume * volume, Some(1));
        }
        let pad = cx.input.pad(0);
        let (held, pressed) = (pad.buttons, pad.pressed);
        let (k, p) = cx.replay.get_input(held, pressed);
        cx.fn_game.keys = k;
        cx.fn_game.pressed = p;
        if cx.fn_game.flags & FnGameState::MUSIC != 0 {
            let seconds = (cx.game.timer / 60).min(127);
            cx.seq_flags[3] = seconds as u8;
        }
        cx.sound.set_seq_flags(&cx.seq_flags);
        actor::move_all(cx);
        if let Some(hook) = cx.frame_hook {
            hook(cx);
        }
    }

    /// Draw (0x41bea0).
    fn draw(&mut self, cx: &mut Cx) {
        actor::draw_all(cx);
    }
}
