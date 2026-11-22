//! The input guide (the port's): one line naming the controls of the
//! screen for the device used last, in a strip of its own along the
//! window's bottom, with the FPS counter at its right end. Display only.
//!
//! The strip is reserved: the menus lay out above it (their logical
//! screen grows by `STRIP` below the 640x480 one), and in a game, where the
//! arena keeps its size, the guide shows only off active play (pause,
//! result, game over, name entry) and the HUD's bottom bands and texts are
//! lifted above it. A dark backing keeps anything that still crosses it
//! (the rotating bands) from running under the text.

use super::Cx;
use super::fn_option::PAD_NAMES;
use super::input::TRIGGER_KEYS;
use crate::gfx::{Argb, Blend, Pass, Primitive, Transform};
use wf_engine::platform::Device;

/// The strip's height (logical units).
pub const STRIP: f32 = 20.0;

/// Which controls the guide names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guide {
    Menu,
    ArenaSelect,
    Options,
    Paused,
    Skip,
    NameEntry,
}

/// When the guide shows (wf-plus.ini `ui.input_guide`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Policy {
    /// While playing by touch (nothing else on screen tells).
    #[default]
    Touch,
    Always,
    Off,
}

impl Policy {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "touch" | "auto" => Some(Policy::Touch),
            "always" => Some(Policy::Always),
            "off" => Some(Policy::Off),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Policy::Touch => "touch",
            Policy::Always => "always",
            Policy::Off => "off",
        }
    }
}

/// The device the guide speaks of.
fn device(cx: &Cx) -> Device {
    cx.settings.guide_device.unwrap_or(cx.ui.device)
}

/// Whether a guide shows now (for a screen that has one).
pub fn shown(cx: &Cx) -> bool {
    match cx.settings.input_guide {
        Policy::Off => false,
        Policy::Always => true,
        Policy::Touch => device(cx) == Device::Touch,
    }
}

/// The strip a menu reserves below its 640x480 screen (0 without a
/// guide). Menus always have one.
pub fn menu_strip(cx: &Cx) -> f32 {
    if cx.draw.menu && shown(cx) { STRIP } else { 0.0 }
}

/// How far the HUD lifts its bottom bands and texts in a game.
pub fn lift(cx: &Cx) -> f32 {
    if !cx.draw.menu && cx.guide.is_some() && shown(cx) { STRIP } else { 0.0 }
}

/// The name of trigger `i`'s key or button.
fn trigger(cx: &Cx, pad: bool, i: usize) -> String {
    let p = cx.input.pad(0);
    if pad {
        PAD_NAMES.get(p.button_map[i] as usize).unwrap_or(&"?").to_string()
    } else {
        let k = TRIGGER_KEYS.iter().position(|&k| k == p.key_map[i]).unwrap_or(0);
        (b"ZXSDAWEF"[k] as char).to_string()
    }
}

/// The guide's text.
pub fn text(cx: &Cx, guide: Guide) -> String {
    let dev = device(cx);
    let pad = dev == Device::Gamepad;
    let (shot, slow, pause) = (trigger(cx, pad, 0), trigger(cx, pad, 1), trigger(cx, pad, 7));
    match (guide, dev) {
        (Guide::Menu, Device::Touch) => "SWIPE UP/DOWN: MENU   TAP: OK   BACK: EXIT".into(),
        (Guide::Menu, Device::Mouse) => "WHEEL OR ARROWS: SELECT   CLICK: OK".into(),
        (Guide::Menu, Device::Gamepad) => format!("D-PAD: SELECT   {shot}: OK"),
        (Guide::Menu, _) => format!("UP/DOWN: SELECT   {shot}: OK   ESC: EXIT"),
        (Guide::ArenaSelect, Device::Touch) => "SWIPE UP/DOWN: ARENA   TAP: START   BACK: MENU".into(),
        (Guide::ArenaSelect, Device::Mouse) => "WHEEL OR ARROWS: ARENA   CLICK: START   RIGHT CLICK: BACK".into(),
        (Guide::ArenaSelect, Device::Gamepad) => format!("D-PAD: ARENA   {shot}: START   {slow}: BACK"),
        (Guide::ArenaSelect, _) => format!("UP/DOWN: ARENA   {shot}: START   {slow}: BACK"),
        (Guide::Options, Device::Touch) => "TAP: SELECT/CHANGE   BACK: TITLE".into(),
        (Guide::Options, Device::Mouse) => "CLICK: SELECT/CHANGE   RIGHT CLICK: BACK A STEP   WHEEL: CHANGE".into(),
        (Guide::Options, Device::Gamepad) => format!("D-PAD: SELECT/CHANGE   {slow}: BACK"),
        (Guide::Options, _) => format!("UP/DOWN: SELECT   LEFT/RIGHT: CHANGE   {slow}: BACK"),
        (Guide::Paused, Device::Touch) => "PAUSED   TAP: RESUME".into(),
        (Guide::Paused, Device::Mouse) => "PAUSED   CLICK: RESUME".into(),
        (Guide::Paused, _) => format!("PAUSED   {pause}: RESUME"),
        (Guide::Skip, Device::Touch) => "TAP: SKIP".into(),
        (Guide::Skip, Device::Mouse) => "CLICK: SKIP".into(),
        (Guide::Skip, _) => format!("{shot}: SKIP"),
        (Guide::NameEntry, Device::Touch) => "SWIPE UP/DOWN: LETTER   TAP: ENTER".into(),
        (Guide::NameEntry, Device::Mouse) => "WHEEL OR CLICK: LETTER   RIGHT CLICK: DELETE".into(),
        (Guide::NameEntry, Device::Gamepad) => format!("D-PAD: LETTER   {shot}: ENTER   {slow}: DELETE"),
        (Guide::NameEntry, _) => format!("ARROWS: LETTER   {shot}: ENTER   {slow}: DELETE"),
    }
}

/// The bottom row, as (left, width, bottom) in logical units: the window
/// on a menu; in a game the HUD's view down to the arena's bottom.
fn row(cx: &Cx) -> (f32, f32, f32) {
    if cx.draw.menu {
        let [x, y, w, h] = cx.window_view();
        (x, w, y + h)
    } else {
        let (left, width) = cx.hud_view();
        (left, width, cx.arena.height)
    }
}

/// Whether boxes `a` and `b` ([x0, y0, x1, y1]) share any area.
pub fn overlap(a: &[f32; 4], b: &[f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// The FPS counter's text: with the perf overlay, the GPU time of a frame
/// ("165 FPS 3.1MS GPU"), or the present-to-present time without GPU
/// timestamps ("165 FPS 6.1MS FRAME").
pub fn fps_text(s: &super::settings::Settings) -> String {
    let fps = format!("{:.0} FPS", s.fps);
    if !s.perf_overlay {
        return fps;
    }
    match s.gpu_ms {
        Some(ms) => format!("{fps} {ms:.1}MS GPU"),
        None => format!("{fps} {:.1}MS FRAME", s.frame_ms),
    }
}

/// Draws the guide and the FPS counter (after the frame's other draws).
pub fn draw(cx: &mut Cx) {
    let fps = (cx.settings.display.show_fps || cx.settings.perf_overlay).then(|| fps_text(&cx.settings));
    let guide = cx.guide.filter(|_| shown(cx)).map(|g| text(cx, g));
    if fps.is_none() && guide.is_none() {
        return;
    }
    let (left, width, bottom) = row(cx);
    let top = bottom - STRIP;
    cx.draw.set_pass(Pass::Overlay);
    if guide.is_some() {
        let quad = [[left, top], [left + width, top], [left, bottom], [left + width, bottom]];
        cx.draw.shape(Primitive::TriangleStrip, &Transform::IDENTITY, &quad, Argb(0xc000_0000), Blend::Normal);
    }
    let mut right = left + width;
    if let Some(f) = &fps {
        let w = 8.0 * f.len() as f32;
        let mut x = left + width - w - 6.0;
        if let Some(corner) = cx.hud.corner
            && corner - w - 16.0 >= left + 6.0
        {
            x = x.min(corner - w - 16.0);
        }
        // Up a row at a time while it would cover a HUD text (the replay's
        // in a narrow arena).
        let mut y = bottom - 14.0;
        let at = |y: f32| [x - 2.0, y - 2.0, x + w + 2.0, y + 10.0];
        while y > 0.0 && cx.hud.boxes.iter().any(|(_, b)| overlap(b, &at(y))) {
            y -= 12.0;
        }
        cx.fonts.small_text.print(&mut cx.draw, x, y, f.as_bytes(), None, Argb(0xa0ff_ffff), -1, Blend::Normal);
        cx.hud.boxes.push(("fps", [x, y, x + w, y + 8.0]));
        if y == bottom - 14.0 {
            right = x - 16.0;
        }
    }
    if let Some(g) = &guide {
        // Centred, or left of the FPS counter when they would meet.
        let w = 8.0 * g.len() as f32;
        let x = (left + (width - w) * 0.5).min(right - w).max(left + 6.0);
        cx.fonts.small_text.print(&mut cx.draw, x, top + 6.0, g.as_bytes(), None, Argb(0xa0ff_ffff), -1, Blend::Normal);
        cx.hud.boxes.push(("guide", [x, top + 6.0, x + w, top + 14.0]));
    }
    cx.draw.set_pass(Pass::Arena);
}

#[cfg(test)]
mod tests {
    use super::super::settings::Settings;

    #[test]
    fn perf_overlay_text() {
        let mut s = Settings { fps: 164.6, ..Default::default() };
        assert_eq!(super::fps_text(&s), "165 FPS");
        s.perf_overlay = true;
        s.frame_ms = 6.07;
        assert_eq!(super::fps_text(&s), "165 FPS 6.1MS FRAME");
        s.gpu_ms = Some(3.12);
        assert_eq!(super::fps_text(&s), "165 FPS 3.1MS GPU");
    }
}
