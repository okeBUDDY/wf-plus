//! The files the game keeps (settings, scores, replays): std::fs, or in
//! the browser the page's store (see `platform::web`), which the page
//! saves in IndexedDB. Read-only game data comes through
//! [`crate::platform::read_file`] instead.

use std::io;
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
pub use std::fs::{copy, create_dir_all, read, read_to_string, remove_file, write};

/// The names of the files and folders in `dir`.
#[cfg(not(target_arch = "wasm32"))]
pub fn list_dir(dir: impl AsRef<Path>) -> io::Result<Vec<String>> {
    Ok(std::fs::read_dir(dir)?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
}

/// A file is at `path`.
#[cfg(not(target_arch = "wasm32"))]
pub fn is_file(path: impl AsRef<Path>) -> bool {
    path.as_ref().is_file()
}

/// When the file at `path` was last written; None when there is none.
#[cfg(not(target_arch = "wasm32"))]
pub fn modified(path: impl AsRef<Path>) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use crate::platform::web;

    pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        web::read_file(path.as_ref())
    }

    pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
        String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn write(path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> io::Result<()> {
        web::write_file(path.as_ref(), data.as_ref().to_vec());
        Ok(())
    }

    /// Folders are implied by the files' paths.
    pub fn create_dir_all(_path: impl AsRef<Path>) -> io::Result<()> {
        Ok(())
    }

    pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        if web::remove_file(path.as_ref()) { Ok(()) } else { Err(io::ErrorKind::NotFound.into()) }
    }

    pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
        let data = read(from)?;
        let n = data.len() as u64;
        write(to, data)?;
        Ok(n)
    }

    pub fn list_dir(dir: impl AsRef<Path>) -> io::Result<Vec<String>> {
        Ok(web::list_dir(dir.as_ref()))
    }

    pub fn is_file(path: impl AsRef<Path>) -> bool {
        web::has_file(path.as_ref())
    }

    pub fn modified(path: impl AsRef<Path>) -> Option<std::time::SystemTime> {
        let ms = web::modified(path.as_ref())?;
        Some(std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64))
    }
}
