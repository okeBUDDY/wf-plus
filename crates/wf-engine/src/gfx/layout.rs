//! The camera: how the arena (the game's logical rect) lands in a window.
//!
//! The arena is fitted whole and centred (its aspect kept, one scale for
//! both axes). The rest of the window shows more of the logical plane
//! around it: the background pass draws there, the arena pass is cut to
//! the arena. A Classic 640x480 arena in a 16:9 window is a 4:3 view with
//! the grid running on into the side margins; an HD arena sized to
//! the window fills it.

/// Where the arena and the visible logical rect are, for one window size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// Window pixels per logical unit.
    pub scale: f32,
    /// The logical rect the window shows: x, y, width, height.
    pub visible: [f32; 4],
    /// The arena in window pixels: x, y, width, height.
    pub arena_px: [u32; 4],
}

impl Layout {
    pub fn new(arena: [f32; 2], size: (u32, u32)) -> Self {
        let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let (aw, ah) = (arena[0].max(1.0), arena[1].max(1.0));
        let scale = (w / aw).min(h / ah).max(1e-3);
        let (vw, vh) = (w / scale, h / scale);
        let visible = [(aw - vw) * 0.5, (ah - vh) * 0.5, vw, vh];
        let px = ((w - aw * scale) * 0.5).floor().max(0.0);
        let py = ((h - ah * scale) * 0.5).floor().max(0.0);
        let pw = (aw * scale).round().min(w - px);
        let ph = (ah * scale).round().min(h - py);
        Layout { scale, visible, arena_px: [px as u32, py as u32, pw as u32, ph as u32] }
    }

    /// The same view on a target of `to` pixels instead of `from` (a
    /// render scale's target, stretched back onto the window): the logical
    /// rect stays the window's, so nothing laid out to the window's edges
    /// moves off them when the target's size rounds to another aspect.
    pub fn resized(self, from: (u32, u32), to: (u32, u32)) -> Self {
        if from == to {
            return self;
        }
        let kx = to.0 as f32 / from.0.max(1) as f32;
        let ky = to.1 as f32 / from.1.max(1) as f32;
        let [x, y, w, h] = self.arena_px;
        let x0 = (x as f32 * kx).round();
        let y0 = (y as f32 * ky).round();
        let x1 = ((x + w) as f32 * kx).round().min(to.0 as f32);
        let y1 = ((y + h) as f32 * ky).round().min(to.1 as f32);
        Layout { scale: self.scale * kx.min(ky), visible: self.visible, arena_px: [x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32] }
    }

    /// The logical point under window pixel (`x`, `y`).
    pub fn to_logical(&self, x: f32, y: f32) -> [f32; 2] {
        [self.visible[0] + x / self.scale, self.visible[1] + y / self.scale]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_in_16_9_is_pillarboxed() {
        let l = Layout::new([640.0, 480.0], (1280, 720));
        assert_eq!(l.scale, 1.5);
        assert_eq!(l.arena_px, [160, 0, 960, 720]);
        let p = l.to_logical(160.0, 0.0);
        assert!(p[0].abs() < 1e-3 && p[1] == 0.0);
        assert_eq!(l.visible[2], 1280.0 / 1.5);
    }

    #[test]
    fn a_resized_layout_keeps_the_window_view() {
        let l = Layout::new([640.0, 480.0], (2000, 1253));
        let r = l.resized((2000, 1253), (1000, 627));
        assert_eq!(r.visible, l.visible);
        assert_eq!(r.arena_px, [82, 0, 836, 627]);
        assert_eq!(l.resized((2000, 1253), (2000, 1253)), l);
    }

    #[test]
    fn a_matching_arena_fills_the_window() {
        let l = Layout::new([853.333_3, 480.0], (1280, 720));
        assert_eq!(l.arena_px[0], 0);
        assert!((l.visible[0]).abs() < 1e-3);
    }
}
