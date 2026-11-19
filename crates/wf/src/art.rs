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
    /// The cooked constant shapes (`gfx::shape_table`).
    pub shapes: Shapes,
}

/// The sprites of the constant shapes.
pub struct Shapes {
    pub hexagon: SpriteId,
    pub menu_cursor: SpriteId,
    pub title_line: SpriteId,
    pub weapon_bar: SpriteId,
    pub backdrop: SpriteId,
    pub name_cursor: SpriteId,
    pub char_cursor: SpriteId,
    pub shockwave: SpriteId,
    pub grid_tile: SpriteId,
    pub screen: SpriteId,
}

impl Art {
    pub fn load() -> Self {
        let pack = pack();
        let s = |name: &str| {
            SpriteId(pack.find(&format!("shape/{name}")).expect("cooked shape") as u32)
        };
        let shapes = Shapes {
            hexagon: s("hexagon"),
            menu_cursor: s("menu_cursor"),
            title_line: s("title_line"),
            weapon_bar: s("weapon_bar"),
            backdrop: s("backdrop"),
            name_cursor: s("name_cursor"),
            char_cursor: s("char_cursor"),
            shockwave: s("shockwave"),
            grid_tile: s("grid_tile"),
            screen: s("screen"),
        };
        Art { pack, shapes }
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
