//! The data the game loads from data.bin at startup (cGlobal::Initialize
//! 0x401880): the sprite patterns' ids and hit boxes, the sound effects and
//! the optional user music.

use crate::data::pack::Pack;
use crate::data::pat::PatFile;
use crate::sound::{SoundBuffer, ogg};
use std::path::Path;

/// The pattern sheets (cGlobal::LoadPatterns 0x401470).
pub const SHEETS: [&str; 6] = ["sys0", "enm0", "add0", "etc0", "etc1", "etc2"];

/// The sound effects and their channels (sounds.h, LoadSounds 0x401550):
/// sound i plays on channel i.
pub const SOUNDS: [(usize, &str); 25] = [
    (0, "bomb_1.ogg"),
    (1, "bomb_2.ogg"),
    (2, "bomb_3.ogg"),
    (3, "bomb_4.ogg"),
    (4, "bomb_pre.ogg"),
    (5, "bomb_all.ogg"),
    (6, "bomb_me.ogg"),
    (7, "damage.ogg"),
    (8, "nodamage.ogg"),
    (10, "warning.ogg"),
    (12, "switch.ogg"),
    (13, "wep0.ogg"),
    (14, "wep1.ogg"),
    (16, "fire_me.ogg"),
    (17, "fire_bl.ogg"),
    (18, "fire_cn.ogg"),
    (19, "fire_ls1.ogg"),
    (20, "fire_ls2.ogg"),
    (21, "fire_ls3.ogg"),
    (22, "fire_ms.ogg"),
    (24, "barrier.ogg"),
    (25, "boost1.ogg"),
    (26, "boost2.ogg"),
    (27, "trans1.ogg"),
    (28, "trans2.ogg"),
];
pub const SOUND_SLOTS: usize = 32;
/// The music's channel (kChannelBgm).
pub const CHANNEL_BGM: usize = 32;
pub const CHANNELS: usize = 40;

pub struct Assets {
    /// The engine's registry with data.bin mounted.
    pub pack: wf_engine::assets::Registry,
    pub pats: Vec<(&'static str, PatFile)>,
    pub sounds: Vec<Option<SoundBuffer>>,
    pub bgm: Option<crate::sound::Music>,
}

impl Assets {
    /// Loads data.bin at `path`; the music is looked for in `home`, then
    /// next to data.bin.
    pub fn load(path: &Path, home: &Path) -> Result<Self, String> {
        let bytes = crate::platform::read_file(path).map_err(|e| format!("data.bin がオープンできません (cannot open {}: {e})", path.display()))?;
        let pack = Pack::from_bytes(bytes).map_err(|e| format!("data.bin がオープンできません ({e})"))?;
        // The user's music, if any: in the home directory (where a phone's
        // user can put it), else next to data.bin.
        let bgm = crate::sound::Music::load(home).or_else(|| path.parent().and_then(crate::sound::Music::load));
        Self::from_pack(pack, bgm)
    }

    /// Loads the data.bin inside the zip at `zip` (the download, read in
    /// place: nothing is saved); the music is looked for in `home`.
    pub fn load_zip(zip: &Path, home: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
        let data = crate::data_import::extract(&bytes).map_err(|e| format!("{}: {e}", zip.display()))?;
        let pack = Pack::from_bytes(data).map_err(|e| format!("data.bin がオープンできません ({e})"))?;
        Self::from_pack(pack, crate::sound::Music::load(home))
    }

    fn from_pack(pack: Pack, bgm: Option<crate::sound::Music>) -> Result<Self, String> {
        let mut pats = Vec::new();
        for sheet in SHEETS {
            let name = format!("{sheet}.pat");
            let bytes = pack.get(&name).ok_or_else(|| format!("{name}がみつかりません."))?;
            pats.push((sheet, PatFile::parse(bytes, 0).ok_or_else(|| format!("{name} is malformed"))?));
        }
        let mut sounds: Vec<Option<SoundBuffer>> = (0..SOUND_SLOTS).map(|_| None).collect();
        for (slot, name) in SOUNDS {
            let bytes = pack.get(name).ok_or_else(|| format!("{name} が見つかりません"))?;
            sounds[slot] = Some(ogg::decode(bytes, 0).map_err(|e| format!("{name}: {e}"))?);
        }
        let mut registry = wf_engine::assets::Registry::new();
        registry.mount(pack);
        Ok(Assets { pack: registry, pats, sounds, bgm })
    }

    pub fn pat(&self, sheet: &str) -> Option<&PatFile> {
        self.pats.iter().find(|(s, _)| *s == sheet).map(|(_, p)| p)
    }
}
