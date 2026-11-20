//! The game data from the player's own download: data.bin taken out of
//! `wf108.zip` (the archive the official website offers), or a bare
//! data.bin, checked to be version 1.08's and saved where the game looks
//! for it. Nothing is downloaded: the player gets the archive from the
//! original website and hands it over (a drop on the window, a file picker,
//! or the archive found in the usual places).

use std::path::{Path, PathBuf};

/// The official download page (the README links the same).
pub const DOWNLOAD_URL: &str = "https://www.hikware.com/Prod/index.html#wf";

/// Version 1.08's data.bin: its size and CRC-32 (as wf108.zip lists it).
pub const DATA_SIZE: usize = 507_240;
pub const DATA_CRC: u32 = 0x309c_1a63;

/// Why a file did not give the game data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// A zip without data.bin.
    NoData,
    /// Neither a zip nor a data.bin.
    NotGameData,
    /// A data.bin, or a zip's, from another version (or damaged).
    WrongVersion,
    /// The zip is damaged or uses a method other than store or deflate.
    BadZip,
    Io(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ImportError::NoData => "THE ZIP HAS NO DATA.BIN",
            ImportError::NotGameData => "NOT WF108.ZIP OR DATA.BIN",
            ImportError::WrongVersion => "NOT THE VERSION 1.08 DATA",
            ImportError::BadZip => "THE ZIP IS DAMAGED",
            ImportError::Io(e) => return write!(f, "CANNOT SAVE DATA.BIN ({})", e.to_ascii_uppercase()),
        })
    }
}

/// CRC-32 (the zip polynomial, reflected).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

/// Whether `data` is version 1.08's data.bin.
pub fn is_game_data(data: &[u8]) -> bool {
    data.len() == DATA_SIZE && crc32(data) == DATA_CRC
}

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// data.bin out of a zip: the entry named data.bin (any case, any folder).
fn from_zip(zip: &[u8]) -> Result<Vec<u8>, ImportError> {
    // The end of central directory record: 22 bytes and a comment of up to
    // 64 KiB at the end.
    let lowest = zip.len().saturating_sub(22 + 0xffff);
    let eocd = (lowest..zip.len().saturating_sub(21))
        .rev()
        .find(|&i| zip[i..].starts_with(b"PK\x05\x06"))
        .ok_or(ImportError::BadZip)?;
    let entries = u16_at(zip, eocd + 10).ok_or(ImportError::BadZip)?;
    let mut at = u32_at(zip, eocd + 16).ok_or(ImportError::BadZip)?;
    for _ in 0..entries {
        if !zip.get(at..).is_some_and(|r| r.starts_with(b"PK\x01\x02")) {
            return Err(ImportError::BadZip);
        }
        let field = |o| u16_at(zip, at + o).ok_or(ImportError::BadZip);
        let (method, name_len, extra_len, comment_len) = (field(10)?, field(28)?, field(30)?, field(32)?);
        let size = u32_at(zip, at + 20).ok_or(ImportError::BadZip)?;
        let local = u32_at(zip, at + 42).ok_or(ImportError::BadZip)?;
        let name = zip.get(at + 46..at + 46 + name_len).ok_or(ImportError::BadZip)?;
        at += 46 + name_len + extra_len + comment_len;
        let base = name.rsplit(|&c| c == b'/' || c == b'\\').next().unwrap_or(name);
        if !base.eq_ignore_ascii_case(b"data.bin") {
            continue;
        }
        if !zip.get(local..).is_some_and(|r| r.starts_with(b"PK\x03\x04")) {
            return Err(ImportError::BadZip);
        }
        let start = local + 30 + u16_at(zip, local + 26).ok_or(ImportError::BadZip)? + u16_at(zip, local + 28).ok_or(ImportError::BadZip)?;
        let raw = zip.get(start..start + size).ok_or(ImportError::BadZip)?;
        let data = match method {
            0 => raw.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, DATA_SIZE + 1).map_err(|_| ImportError::BadZip)?,
            _ => return Err(ImportError::BadZip),
        };
        return if is_game_data(&data) { Ok(data) } else { Err(ImportError::WrongVersion) };
    }
    Err(ImportError::NoData)
}

/// The game data in `bytes`: a zip holding data.bin, or data.bin itself.
pub fn extract(bytes: &[u8]) -> Result<Vec<u8>, ImportError> {
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return from_zip(bytes);
    }
    if is_game_data(bytes) {
        return Ok(bytes.to_vec());
    }
    // data.bin is a pack whose files are named in its header; anything of
    // about its size that is not 1.08's is another version's.
    if bytes.len().abs_diff(DATA_SIZE) < DATA_SIZE / 2 && bytes.windows(8).any(|w| w == b"plan.dat") {
        return Err(ImportError::WrongVersion);
    }
    Err(ImportError::NotGameData)
}

/// Takes the game data out of `bytes` and saves it as `dest` (written
/// beside it first, then renamed, so a failure leaves no half file).
pub fn install(bytes: &[u8], dest: &Path) -> Result<(), ImportError> {
    let data = extract(bytes)?;
    let io = |e: std::io::Error| ImportError::Io(e.kind().to_string());
    if let Some(dir) = dest.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let tmp = dest.with_extension("bin.part");
    std::fs::write(&tmp, &data).map_err(io)?;
    std::fs::rename(&tmp, dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(e)
    })
}

/// [`install`] from a file.
pub fn install_file(path: &Path, dest: &Path) -> Result<(), ImportError> {
    // wf108.zip is under 1 MiB; refuse anything far bigger unread.
    let len = std::fs::metadata(path).map_err(|e| ImportError::Io(e.kind().to_string()))?.len();
    if len > 64 << 20 {
        return Err(ImportError::NotGameData);
    }
    install(&std::fs::read(path).map_err(|e| ImportError::Io(e.kind().to_string()))?, dest)
}

/// Whether a file name looks like the download: wf108.zip, as browsers
/// save it again ("wf108 (1).zip", "wf108-1.zip"), or data.bin.
fn is_candidate(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "data.bin" || (n.starts_with("wf108") && n.ends_with(".zip"))
}

/// The folders the game data is looked for in: the working directory,
/// beside the executable, and the user's Downloads folder.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(dir) = std::env::current_dir() {
        dirs.push(dir);
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        dirs.push(dir);
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    if let Some(home) = home {
        dirs.push(home.join("Downloads"));
    }
    let mut seen = Vec::new();
    dirs.retain(|d| {
        let key = std::fs::canonicalize(d).unwrap_or_else(|_| d.clone());
        let new = !seen.contains(&key);
        seen.push(key);
        new
    });
    dirs
}

/// The files in `dirs` that may be the game data: every data.bin first (in
/// the order of `dirs`), then the zips, newest first.
pub fn candidates(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: Vec<(bool, std::cmp::Reverse<std::time::SystemTime>, PathBuf)> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name();
            if !name.to_str().is_some_and(is_candidate) {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_file() && meta.len() <= 64 << 20 {
                let zip = !name.eq_ignore_ascii_case("data.bin");
                // data.bin keeps the folders' order (the sort is stable).
                let at = if zip { meta.modified().unwrap_or(std::time::UNIX_EPOCH) } else { std::time::UNIX_EPOCH };
                found.push((zip, std::cmp::Reverse(at), e.path()));
            }
        }
    }
    found.sort_by_key(|a| (a.0, a.1));
    found.into_iter().map(|(.., p)| p).collect()
}

/// The first of `candidates` that installs, if any.
pub fn install_found(candidates: &[PathBuf], dest: &Path) -> Option<PathBuf> {
    candidates.iter().find(|p| p.as_path() != dest && install_file(p, dest).is_ok()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_zip_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    /// A zip with one entry, stored or deflated.
    fn zip(name: &str, data: &[u8], deflate: bool) -> Vec<u8> {
        let body = if deflate { miniz_oxide::deflate::compress_to_vec(data, 6) } else { data.to_vec() };
        let method: u16 = if deflate { 8 } else { 0 };
        let mut z = Vec::new();
        let local = |z: &mut Vec<u8>, sig: &[u8]| {
            z.extend_from_slice(sig);
        };
        local(&mut z, b"PK\x03\x04");
        z.extend_from_slice(&[20, 0, 0, 0]);
        z.extend_from_slice(&method.to_le_bytes());
        z.extend_from_slice(&[0; 4]);
        z.extend_from_slice(&crc32(data).to_le_bytes());
        z.extend_from_slice(&(body.len() as u32).to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&[0, 0]);
        z.extend_from_slice(name.as_bytes());
        z.extend_from_slice(&body);
        let cd = z.len();
        z.extend_from_slice(b"PK\x01\x02");
        z.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
        z.extend_from_slice(&method.to_le_bytes());
        z.extend_from_slice(&[0; 4]);
        z.extend_from_slice(&crc32(data).to_le_bytes());
        z.extend_from_slice(&(body.len() as u32).to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&[0; 12]);
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(name.as_bytes());
        let cd_len = z.len() - cd;
        z.extend_from_slice(b"PK\x05\x06");
        z.extend_from_slice(&[0; 4]);
        z.extend_from_slice(&[1, 0, 1, 0]);
        z.extend_from_slice(&(cd_len as u32).to_le_bytes());
        z.extend_from_slice(&(cd as u32).to_le_bytes());
        z.extend_from_slice(&[0, 0]);
        z
    }

    #[test]
    fn rejects_what_is_not_the_data() {
        assert_eq!(extract(b"hello"), Err(ImportError::NotGameData));
        assert_eq!(extract(&zip("readme.txt", b"hi", false)), Err(ImportError::NoData));
        assert_eq!(extract(&zip("wf108/DATA.BIN", b"not it", true)), Err(ImportError::WrongVersion));
        assert_eq!(extract(b"PK\x03\x04 truncated"), Err(ImportError::BadZip));
        let mut other = vec![0u8; DATA_SIZE];
        other[100..108].copy_from_slice(b"plan.dat");
        assert_eq!(extract(&other), Err(ImportError::WrongVersion));
    }

    #[test]
    fn finds_download_names() {
        for n in ["wf108.zip", "WF108.ZIP", "wf108 (1).zip", "wf108-2.zip", "data.bin"] {
            assert!(is_candidate(n), "{n}");
        }
        for n in ["wf107.zip", "data.bin.part", "wf108.zip.crdownload"] {
            assert!(!is_candidate(n), "{n}");
        }
    }

    /// With the real download (WF_ZIP, or the project's copy): data.bin
    /// comes out of it and installs.
    #[test]
    fn imports_the_real_download() {
        let path = std::env::var_os("WF_ZIP")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../software/warning-forever/1.08/wf108.zip")));
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("skipped: no {}", path.display());
            return;
        };
        let data = extract(&bytes).expect("data.bin from wf108.zip");
        assert!(is_game_data(&data));
        assert_eq!(extract(&data).as_deref(), Ok(&data[..]));
        // The same, stored rather than deflated, in a folder.
        assert_eq!(extract(&zip("Warning Forever/data.bin", &data, false)).as_deref(), Ok(&data[..]));
        let dir = std::env::temp_dir().join(format!("wf-import-{}", std::process::id()));
        let dest = dir.join("home").join("data.bin");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("wf108 (1).zip"), &bytes).unwrap();
        std::fs::write(dir.join("wf108.zip"), &bytes).unwrap();
        std::fs::write(dir.join("DATA.BIN"), &data).unwrap();
        // data.bin before any zip; then the newest zip.
        let c = candidates(std::slice::from_ref(&dir));
        assert_eq!(c[0], dir.join("DATA.BIN"));
        assert_eq!(c.len(), 3);
        std::fs::remove_file(dir.join("DATA.BIN")).unwrap();
        std::fs::remove_file(dir.join("wf108.zip")).unwrap();
        let found = candidates(std::slice::from_ref(&dir));
        assert_eq!(install_found(&found, &dest), Some(dir.join("wf108 (1).zip")));
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
