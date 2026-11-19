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
