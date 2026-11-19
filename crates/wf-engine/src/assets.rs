//! The asset registry: named, read-only byte assets from mounted sources
//! (a game's packed data file, a directory). A lookup asks the sources
//! newest-mounted first. The cooked art is separate: it is GPU data the
//! renderer owns (`wf_art::Pack`).

use std::collections::HashMap;
use std::path::Path;

/// Where assets come from.
pub trait AssetSource: Send {
    fn get(&self, name: &str) -> Option<&[u8]>;
    fn names(&self) -> Vec<String>;
}

/// The files of a directory (not its subdirectories), read whole.
pub struct DirSource {
    files: HashMap<String, Vec<u8>>,
}

impl DirSource {
    pub fn open(dir: &Path) -> std::io::Result<Self> {
        let mut files = HashMap::new();
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            if e.file_type()?.is_file() {
                files.insert(e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path())?);
            }
        }
        Ok(DirSource { files })
    }
}

impl AssetSource for DirSource {
    fn get(&self, name: &str) -> Option<&[u8]> {
        self.files.get(name).map(Vec::as_slice)
    }
    fn names(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }
}

/// In-memory assets (tests and tools).
#[derive(Default)]
pub struct MemorySource(pub HashMap<String, Vec<u8>>);

impl AssetSource for MemorySource {
    fn get(&self, name: &str) -> Option<&[u8]> {
        self.0.get(name).map(Vec::as_slice)
    }
    fn names(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}

#[derive(Default)]
pub struct Registry {
    sources: Vec<Box<dyn AssetSource>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `source` over the ones mounted before it.
    pub fn mount(&mut self, source: impl AssetSource + 'static) {
        self.sources.push(Box::new(source));
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.sources.iter().rev().find_map(|s| s.get(name))
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Every name, once, sorted.
    pub fn names(&self) -> Vec<String> {
        let mut n: Vec<String> = self.sources.iter().flat_map(|s| s.names()).collect();
        n.sort();
        n.dedup();
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_mount_wins() {
        let mut r = Registry::new();
        let mut a = MemorySource::default();
        a.0.insert("x".into(), vec![1]);
        a.0.insert("y".into(), vec![2]);
        let mut b = MemorySource::default();
        b.0.insert("x".into(), vec![3]);
        r.mount(a);
        r.mount(b);
        assert_eq!(r.get("x"), Some(&[3u8][..]));
        assert_eq!(r.get("y"), Some(&[2u8][..]));
        assert_eq!(r.get("z"), None);
        assert_eq!(r.names(), ["x", "y"]);
    }
}
