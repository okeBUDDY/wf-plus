//! The records (replays) of cGlobal and the rating of a finished game
//! (record.cpp). Replays live in `rec/` under the game's home directory.

use super::Cx;
use super::replay;
use std::path::PathBuf;

/// The record being written ("rec\\temp.rec").
pub const TEMP: &str = "rec/temp.rec";

impl Cx {
    /// A path under the game's home directory (the original's working
    /// directory).
    pub fn home_path(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }
}

/// cGlobal::InitRecords (0x4186f0).
pub fn init_records(cx: &mut Cx) {
    cx.global.replay_path.clear();
    cx.global.last_record.clear();
    let _ = wf_engine::fs::create_dir_all(cx.home_path("rec"));
}

/// cGlobal::ClearReplay (0x418710).
pub fn clear_replay(cx: &mut Cx) {
    cx.global.replay_path.clear();
}

fn copy(cx: &Cx, from: &str, to: &str) {
    let _ = wf_engine::fs::copy(cx.home_path(from), cx.home_path(to));
}

/// cGlobal::NextReplay (0x418720): the replay to play next: the last
/// record if it is one, else the first replay after replay_path by name,
/// wrapping around.
pub fn next_replay(cx: &mut Cx) -> Option<String> {
    let _ = wf_engine::fs::remove_file(cx.home_path(TEMP));
    if !cx.global.last_record.is_empty() {
        let path = std::mem::take(&mut cx.global.last_record);
        if replay::is_replay_file(&cx.home_path(&path)) {
            cx.global.replay_path = path.clone();
            return Some(path);
        }
    }
    let dir = wf_engine::fs::list_dir(cx.home_path("rec")).ok()?;
    let mut next = String::new();
    let mut first = String::new();
    for name in dir {
        // FindFirstFile("rec\\*.rec"), names shorter than 0x40.
        if name.len() >= 0x40 || !name.to_ascii_lowercase().ends_with(".rec") {
            continue;
        }
        let file = format!("rec/{name}");
        if !replay::is_replay_file(&cx.home_path(&file)) {
            continue;
        }
        if first.is_empty() || first > file {
            first = file.clone();
        }
        let after = &cx.global.replay_path;
        if (after.is_empty() || after.as_str() < file.as_str()) && (next.is_empty() || next >= file) {
            next = file;
        }
    }
    let path = if !next.is_empty() {
        next
    } else if !first.is_empty() {
        first
    } else {
        return None;
    };
    cx.global.replay_path = path.clone();
    Some(path)
}

/// cGlobal::SaveReplay (0x418a00): keeps the record just made: as the
/// boss's replay for the lone titles, and when `finished`, as the best
/// (on a new high score) and the last one.
pub fn save_replay(cx: &mut Cx, finished: bool) {
    let title = cx.enm.state.title;
    if title.weapon == -1 && title.variant == -1 && title.words[1] == -1 && title.words[0] >= 14 {
        let path = format!("rec/boss{:02}.rec", title.words[0] - 14);
        copy(cx, TEMP, &path);
    }
    if finished {
        if cx.scores.rank() == 0 {
            copy(cx, TEMP, "rec/best.rec");
        }
        cx.global.last_record = "rec/last.rec".to_string();
        copy(cx, TEMP, "rec/last.rec");
    }
    if cx.global.flags & super::game_state::FLAG_STAGE_REPLAYS != 0 {
        let path = format!("rec/stage{:02}.rec", cx.global.stage);
        copy(cx, TEMP, &path);
        cx.global.last_record = "rec/stage01.rec".to_string();
    }
    let _ = wf_engine::fs::remove_file(cx.home_path(TEMP));
}

/// FnGame::RateGame (0x418b60): rates the game ending at this stage.
pub fn rate_game(cx: &mut Cx) {
    let s = cx.enm.state.sides;
    let word = cx.enm.state.title.words[0] as i32 - 13;
    let (grade, title) = if s[0].mode == 0 && s[1].mode == 0 {
        (0, 0)
    } else if s[0].mode == 0 || s[1].mode == 0 {
        (1, 0)
    } else if s[0].mode == 3 || s[1].mode == 3 {
        if s[0].progress >= 100 && s[1].progress >= 100 { (6, word) } else { (5, word) }
    } else if s[0].progress >= 100 && s[1].progress >= 100 {
        (5, word)
    } else if s[0].progress >= 65 {
        ((s[1].progress >= 65) as i32 + 3, 0)
    } else {
        ((s[1].progress >= 65) as i32 + 2, 0)
    };
    cx.global.rating = super::game_state::GameRating { stage: cx.global.stage as i8, grade: grade as i8, title: title as i8 };
    // g_rating shares its address with g_seq_flags (0x468fa0): the music
    // reads the rating as its flags 0x10..0x12.
    cx.seq_flags[0] = cx.global.stage as u8;
    cx.seq_flags[1] = grade as u8;
    cx.seq_flags[2] = title as u8;
}
