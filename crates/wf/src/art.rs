//! The cooked art, built into the executable by build.rs.

use crate::gfx::SpriteId;
use wf_art::Pack;

static BLOB: &[u8] = wf_art::include_aligned!(concat!(env!("OUT_DIR"), "/art.wfa"));

pub fn pack() -> Pack<'static> {
    Pack::parse(BLOB).expect("the built-in art pack is valid")
}

/// Sprite lookups by name.
pub struct Art {
    pub pack: Pack<'static>,
}

impl Art {
    pub fn load() -> Self {
        Art { pack: pack() }
    }

    pub fn find(&self, name: &str) -> Option<SpriteId> {
        self.pack.find(name).map(|i| SpriteId(i as u32))
    }

    /// The sprite of pattern `id` of `sheet` (e.g. "enm0", 1).
    pub fn pattern(&self, sheet: &str, id: u32) -> Option<SpriteId> {
        self.find(&format!("{sheet}_{id:03}"))
    }

    pub fn name(&self, id: SpriteId) -> &'static str {
        self.pack.name(&self.pack.sprites[id.0 as usize])
    }

    pub fn count(&self) -> usize {
        self.pack.sprites.len()
    }
}
