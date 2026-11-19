//! The state of the running game: Game (game.cpp: the clock, the time
//! bonus, the stock) and the parts of cGlobal (cglobal.h) the game uses.

use super::Cx;
use super::actor::{self, Aid, Param};

/// GameRules: the rule set chosen on the title screen.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GameRules {
    /// 0: time bonuses are given.
    pub mode: i16,
    /// Seconds, 0: no limit.
    pub time_limit: i16,
    /// Player stock, 0: none.
    pub stock: i16,
    /// EXTEND_*.
    pub extend: i16,
}

pub const EXTEND_NONE: i16 = 0;
pub const EXTEND_BOSS: i16 = 1;
pub const EXTEND_PARTS: i16 = 2;

impl Default for GameRules {
    /// Game::GetDefaultRules (0x41eca0).
    fn default() -> Self {
        GameRules { mode: 0, time_limit: 180, stock: 0, extend: EXTEND_NONE }
    }
}

/// Game::Flags.
pub const COUNT_UP: u32 = 0x1;
pub const TIME_BONUS: u32 = 0x2;
pub const RUNNING: u32 = 0x4;
pub const GAME_OVER: u32 = 0x8;
pub const MAX_TIMER: i32 = 999 * 60;

/// Game (g_game) with g_player and g_boss.
#[derive(Clone, Debug, Default)]
pub struct Game {
    pub flags: u32,
    /// Frames left (elapsed with COUNT_UP).
    pub timer: i32,
    /// Frames still to add to the clock.
    pub bonus: i32,
    /// Frames played.
    pub frames: i32,
    /// -1 without stock.
    pub lives: i16,
    /// Parts destroyed toward the next extend.
    pub extend_count: i16,
    pub rules: GameRules,
    /// g_player.
    pub player: Option<Aid>,
    /// g_boss: the boss body, then its explosion (control 20).
    pub boss: Option<Aid>,
}

impl Game {
    /// Game::Start (0x41ecc0).
    pub fn start(&mut self, rules: &GameRules) {
        self.rules = *rules;
        self.flags = 0;
        self.extend_count = -1;
        if self.rules.time_limit != 0 {
            if self.rules.mode == 0 {
                self.flags = TIME_BONUS;
            }
            self.timer = self.rules.time_limit as i32 * 60;
        } else {
            self.flags = COUNT_UP;
            if self.rules.stock == 0 {
                self.rules.stock = 1;
            }
            if self.rules.extend == EXTEND_PARTS {
                self.extend_count = 0;
            }
            self.timer = 0;
        }
        self.lives = if self.rules.stock > 0 { self.rules.stock } else { -1 };
        self.bonus = 0;
        self.frames = 0;
    }

    /// Game::SetLives (0x41ed50).
    pub fn set_lives(&mut self, lives: i16, extend_count: i16) {
        if self.lives >= 0 {
            self.lives = lives;
        }
        if self.extend_count >= 0 {
            self.extend_count = extend_count;
        }
    }

    /// Game::Tick (0x41ed80).
    pub fn tick(&mut self) {
        if self.flags & COUNT_UP != 0 {
            if self.flags & RUNNING != 0 {
                self.timer += 1;
                self.frames += 1;
            }
        } else {
            if self.flags & RUNNING != 0 {
                self.timer -= 1;
                self.frames += 1;
            }
            if self.flags & TIME_BONUS != 0 && self.bonus != 0 {
                if self.bonus > 0 {
                    self.bonus -= 20;
                    self.timer += 20;
                } else {
                    self.bonus += 20;
                    self.timer -= 20;
                }
            }
            if self.timer <= 0 {
                self.timer = 0;
                self.flags |= GAME_OVER;
            }
        }
        if self.timer > MAX_TIMER {
            self.timer = MAX_TIMER;
        }
    }

    /// Game::OnPlayerDestroyed (0x41ee60).
    pub fn on_player_destroyed(&mut self) {
        if self.flags & TIME_BONUS != 0 {
            self.bonus -= 20 * 60;
        }
        self.player = None;
    }

    /// Game::OnPartDestroyed (0x41ee80).
    pub fn on_part_destroyed(&mut self) {
        if self.rules.stock != 0 && self.rules.extend == EXTEND_PARTS {
            self.extend_count += 1;
            if self.extend_count == 100 {
                if self.lives < 9 {
                    self.lives += 1;
                }
                self.extend_count = 0;
            }
        }
    }

    /// Game::OnBossDestroyed (0x41eec0).
    pub fn on_boss_destroyed(&mut self) {
        if self.flags & TIME_BONUS != 0 {
            self.bonus += 30 * 60;
        }
        if self.rules.stock != 0 && self.rules.extend == EXTEND_BOSS && self.lives < 9 {
            self.lives += 1;
        }
    }
}

/// Game::SpawnPlayer (0x41ee00): uses up a life; ends the game without one.
pub fn spawn_player(cx: &mut Cx) {
    let g = &mut cx.game;
    if g.rules.stock != 0 {
        if g.lives <= 0 {
            g.flags |= GAME_OVER;
            if g.extend_count > 0 {
                g.extend_count = 0;
            }
            return;
        }
        g.lives -= 1;
    } else if g.flags & GAME_OVER != 0 {
        return;
    }
    cx.game.player = actor::create(cx, -1, None, 320.0, 400.0, 8, 0, Param::None);
}

/// A score of the score table (ScoreEntry).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ScoreEntry {
    pub score: u32,
    /// Seconds (frames while the game runs).
    pub time: u32,
    pub stage: i16,
    pub deaths: i16,
    pub name: [u8; 4],
}

/// cGlobal::Flags.
pub const FLAG_STAGE_REPLAYS: u32 = 0x1;
pub const FLAG_PAUSE: u32 = 0x2;
pub const FLAG_ENGLISH: u32 = 0x4;

/// The rating of the last game (GameRating, g_rating).
#[derive(Clone, Copy, Debug, Default)]
pub struct GameRating {
    pub stage: i8,
    pub grade: i8,
    pub title: i8,
}

/// cGlobal (g_global): the state of the current game and the settings.
#[derive(Clone, Debug)]
pub struct Global {
    pub flags: u32,
    /// The current stage (from 0).
    pub stage: i32,
    pub rules: GameRules,
    pub entry: ScoreEntry,
    pub effect_volume: f32,
    pub music_volume: f32,
    /// The replay to play, "" for none.
    pub replay_path: String,
    /// Where the last game was saved.
    pub last_record: String,
    pub rating: GameRating,
}

impl Default for Global {
    fn default() -> Self {
        Global {
            flags: 0,
            stage: 0,
            rules: GameRules::default(),
            entry: ScoreEntry::default(),
            effect_volume: 1.0,
            music_volume: 1.0,
            replay_path: String::new(),
            last_record: String::new(),
            rating: GameRating::default(),
        }
    }
}
