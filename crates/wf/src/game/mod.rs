//! The game layer: the application loop (Application::Run), its task lists
//! and stages, and the services they share. Platform independent: the
//! frame goes out as a `DrawList`, sound through the mixer.

pub mod app;
pub mod assets;
pub mod fade;
pub mod input;
pub mod task;
pub mod text;
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
    /// The overlay that flashes and fades the screen (g_global.screen_fade).
    pub screen_fade: Option<task::TaskId>,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A context without data.bin (no sounds, no patterns).
    pub fn cx() -> Cx {
        let art = Art::load();
        let fonts = Fonts::load(&art);
        Cx {
            draw: DrawList::new(),
            art,
            assets: Assets {
                pack: crate::data::pack::Pack::from_bytes(vec![0; 8]).unwrap(),
                pats: Vec::new(),
                sounds: Vec::new(),
                bgm: None,
            },
            fonts,
            input: Input::default(),
            sound: SoundSystem::new(4),
            tasks: Default::default(),
            overlays: Default::default(),
            app: Default::default(),
            screen_fade: None,
        }
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
