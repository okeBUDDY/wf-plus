//! Sprite pattern files (*.pat, read by HLSPatFile::Load 0x40a320). The art
//! of each pattern is cooked separately; what the game still needs from the
//! .pat is which ids exist and their hit boxes.

#[derive(Clone, Copy, Debug, Default)]
pub struct Pattern {
    pub id: i32,
    /// Source rect in the original sheet (w = h = 0: collision only).
    pub rect: [i16; 4],
    /// The pivot inside the rect.
    pub origin: [i32; 2],
    /// Half the hit box size, as the game stores it.
    pub hit_half: [u16; 2],
}

/// The patterns of one sheet, indexed by id.
#[derive(Clone, Debug, Default)]
pub struct PatFile {
    pub patterns: Vec<Option<Pattern>>,
}

impl PatFile {
    /// Parses `bytes`, with room for at least `min_count` ids.
    pub fn parse(bytes: &[u8], min_count: usize) -> Option<Self> {
        let i32_at = |at: usize| bytes.get(at..at + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()));
        let i16_at = |at: usize| bytes.get(at..at + 2).map(|b| i16::from_le_bytes(b.try_into().unwrap()));
        let count = i32_at(0)?.max(0) as usize;
        let slots = ((i32_at(4)? + 1).max(0) as usize).max(min_count);
        let mut patterns = vec![None; slots];
        for i in 0..count {
            let r = 8 + i * 24;
            let id = i32_at(r)?;
            let p = Pattern {
                id,
                rect: [i16_at(r + 4)?, i16_at(r + 6)?, i16_at(r + 8)?, i16_at(r + 10)?],
                origin: [i32_at(r + 12)?, i32_at(r + 16)?],
                hit_half: [(i16_at(r + 20)? as i32 >> 1) as u16, (i16_at(r + 22)? as i32 >> 1) as u16],
            };
            if id >= 0 && (id as usize) < slots {
                patterns[id as usize] = Some(p);
            }
        }
        Some(PatFile { patterns })
    }

    pub fn get(&self, id: usize) -> Option<&Pattern> {
        self.patterns.get(id).and_then(Option::as_ref)
    }
}
