//! data.bin (pack.cpp, 0x40dbb0-0x40de66): a directory of named members,
//! read whole into memory. Names match case-sensitively and there is no
//! loose-file fallback, as in the original.

use std::collections::HashMap;
use std::path::Path;

pub struct Pack {
    data: Vec<u8>,
    entries: HashMap<String, (usize, usize)>,
}

#[derive(Debug)]
pub struct PackError(pub String);

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PackError {}

fn u32_at(d: &[u8], at: usize) -> Result<u32, PackError> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| PackError("data.bin is truncated".into()))
}

impl Pack {
    pub fn open(path: &Path) -> Result<Self, PackError> {
        let data = std::fs::read(path).map_err(|e| PackError(format!("cannot open {}: {e}", path.display())))?;
        Self::from_bytes(data)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self, PackError> {
        let count = u32_at(&data, 0)? as usize;
        let names_size = u32_at(&data, 4)? as usize;
        let names_at = 8 + count * 12;
        let names = data
            .get(names_at..names_at + names_size)
            .ok_or_else(|| PackError("data.bin name table out of range".into()))?;
        let mut entries = HashMap::with_capacity(count);
        for i in 0..count {
            let e = 8 + i * 12;
            let name_offset = u32_at(&data, e)? as usize;
            let offset = u32_at(&data, e + 4)? as usize;
            let size = u32_at(&data, e + 8)? as usize;
            let name = names.get(name_offset..).unwrap_or(&[]);
            let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            let name = String::from_utf8_lossy(&name[..end]).into_owned();
            if offset + size > data.len() {
                return Err(PackError(format!("data.bin member {name} out of range")));
            }
            // PackFile::Open (0x40dbb0): the first entry of a name wins.
            entries.entry(name).or_insert((offset, size));
        }
        Ok(Pack { data, entries })
    }

    /// The bytes of member `name`.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.entries.get(name).map(|&(o, s)| &self.data[o..o + s])
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
}
