//! The arena: the playfield the game runs in, and every gameplay value
//! that derives from its size, in one place.
//!
//! * Classic is the original's 640x480 screen; its values are the
//!   original's constants, bit for bit, so its replays and scores are the
//!   original's.
//! * HD is 480 high and as wide as the window's aspect when the game
//!   starts (270..1920, see `wf_engine::app::ARENA_WIDTH_RANGE`), locked for
//!   the game and kept in its recordings.
//!
//! The height is 480 in both, so the vertical values never change; the
//! horizontal ones are written as offsets from the original's edges and
//! centre (e.g. the player's right bound is 16 in from the right edge, as
//! 624 is in 640).

/// Which arena the rules pick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Classic,
    Hd,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arena {
    pub mode: Mode,
    pub width: f32,
    pub height: f32,
}

impl Default for Arena {
    fn default() -> Self {
        Arena::CLASSIC
    }
}

/// The original's screen.
pub const W: f32 = 640.0;

impl Arena {
    pub const CLASSIC: Arena = Arena { mode: Mode::Classic, width: W, height: 480.0 };

    /// An HD arena `width` wide (whole units, clamped).
    pub fn hd(width: f32) -> Arena {
        let (lo, hi) = wf_engine::app::ARENA_WIDTH_RANGE;
        Arena { mode: Mode::Hd, width: width.round().clamp(lo, hi), height: 480.0 }
    }

    pub fn is_classic(&self) -> bool {
        self.mode == Mode::Classic
    }

    /// The horizontal centre (320 in Classic).
    pub fn center_x(&self) -> f32 {
        if self.is_classic() { 320.0 } else { self.width * 0.5 }
    }

    /// `x` (an original screen x) moved with the right edge: 624 -> w - 16.
    pub fn right_x(&self, x: f32) -> f32 {
        if self.is_classic() { x } else { x - W + self.width }
    }

    /// `x` (an original screen x) moved with the centre: 320 -> w / 2.
    pub fn center_offset_x(&self, x: f32) -> f32 {
        if self.is_classic() { x } else { x - 320.0 + self.width * 0.5 }
    }

    /// A horizontal spread of the original's, widened with the arena.
    pub fn spread(&self, dx: f32) -> f32 {
        if self.is_classic() { dx } else { dx * (self.width / W) }
    }

    /// Control01's bounds (left, top, right, bottom).
    pub fn player_bounds(&self) -> [f32; 4] {
        [16.0, 32.0, self.right_x(624.0), 448.0]
    }

    /// Control32::bounds_ of a new game: left, top, right, bottom.
    pub fn boss_bounds(&self) -> [f32; 4] {
        [64.0, 64.0, self.right_x(576.0), 416.0]
    }

    /// The actor system's culling rect (100 beyond the sides, 32 above,
    /// 32 below).
    pub fn cull(&self) -> [f32; 4] {
        [-100.0, -32.0, self.right_x(740.0), 512.0]
    }

    /// The hit image's size.
    pub fn hit_size(&self) -> (i32, i32) {
        (self.width as i32, self.height as i32)
    }

    /// The width and height a recording stores.
    pub fn size_u16(&self) -> [u16; 2] {
        [self.width as u16, self.height as u16]
    }
}

/// Reads the last arena choice from the port's settings (wf-plus.ini).
pub fn load_choice(cx: &mut super::Cx) {
    cx.arena_choice = match cx.port.get("game.arena") {
        Some(v) if v.eq_ignore_ascii_case("hd") => Mode::Hd,
        _ => Mode::Classic,
    };
}

/// Remembers the arena choice.
pub fn save_choice(cx: &mut super::Cx) {
    let v = if cx.arena_choice == Mode::Hd { "hd" } else { "classic" };
    cx.port.set("game.arena", v);
    if let Err(e) = cx.port.save() {
        crate::log::warn(&format!("cannot save wf-plus.ini: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_is_the_original() {
        let a = Arena::CLASSIC;
        assert_eq!(a.player_bounds(), [16.0, 32.0, 624.0, 448.0]);
        assert_eq!(a.boss_bounds(), [64.0, 64.0, 576.0, 416.0]);
        assert_eq!(a.cull(), [-100.0, -32.0, 740.0, 512.0]);
        assert_eq!(a.center_x(), 320.0);
        assert_eq!(a.spread(160.0), 160.0);
    }

    #[test]
    fn hd_widens_from_the_edges() {
        let a = Arena::hd(853.3);
        assert_eq!(a.width, 853.0);
        assert_eq!(a.player_bounds()[2], 837.0);
        assert_eq!(a.center_x(), 426.5);
        assert_eq!(Arena::hd(100.0).width, 270.0);
    }
}
