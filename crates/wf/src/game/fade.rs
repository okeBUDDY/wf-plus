//! The screen fade (Sprite, sprite.cpp): a full-screen colour drawn over the
//! frame from Application::overlays. g_global.screen_fade uses it to flash
//! and fade the screen.

use super::Cx;
use super::task::Task;
use crate::gfx::Argb;

#[derive(Default)]
pub struct ScreenFade {
    rgba: [i16; 4],
    from: [i16; 4],
    delta: [i16; 4],
    frames: i16,
    time: i16,
}

impl ScreenFade {
    /// SetColor: sets the colour (0..255 per channel) and stops the fade.
    pub fn set_color(&mut self, r: i16, g: i16, b: i16, a: i16) {
        self.rgba = [r, g, b, a];
        self.frames = 0;
    }

    /// FadeTo (0x41bca0).
    pub fn fade_to(&mut self, frames: i16, r: i16, g: i16, b: i16, a: i16) {
        self.from = self.rgba;
        self.time = 0;
        self.delta = [r - self.rgba[0], g - self.rgba[1], b - self.rgba[2], a - self.rgba[3]];
        self.frames = frames;
    }

    /// FadeIn (0x41beb0): from black.
    pub fn fade_in(&mut self, frames: i16) {
        self.set_color(0, 0, 0, 255);
        self.fade_to(frames, 0, 0, 0, 0);
    }

    /// FadeOut (0x41bf00): from clear to black.
    pub fn fade_out(&mut self, frames: i16) {
        self.set_color(0, 0, 0, 0);
        self.fade_to(frames, 0, 0, 0, 255);
    }

    pub fn fade_frames(&self) -> i16 {
        self.frames
    }

    fn color(&self) -> Argb {
        let c = |v: i16| (v as i32).clamp(0, 255) as u8;
        Argb::from_rgba(c(self.rgba[0]), c(self.rgba[1]), c(self.rgba[2]), c(self.rgba[3]))
    }
}

impl Task for ScreenFade {
    fn initialize(&mut self, _cx: &mut Cx) -> bool {
        self.set_color(0, 0, 0, 0);
        self.time = 0;
        true
    }

    /// Advances the fade (0x41bb60); the sweep calls it every frame.
    fn is_protected(&mut self, _cx: &mut Cx) -> bool {
        if self.frames != 0 {
            self.time += 1;
            if self.time >= self.frames {
                for i in 0..4 {
                    self.rgba[i] = self.from[i] + self.delta[i];
                }
                self.time = 0;
                self.frames = 0;
            } else {
                for i in 0..4 {
                    // Integer maths as in the original (truncating division).
                    self.rgba[i] = (self.from[i] as i32 + self.delta[i] as i32 * self.time as i32 / self.frames as i32) as i16;
                }
            }
        }
        true
    }

    fn run(&mut self, cx: &mut Cx) {
        if self.rgba[3] != 0 {
            cx.draw.fill_screen(self.color());
        }
    }
}
