//! Profile (profile.h): the game's settings file, the portable equivalent
//! of wf.ini. The same [Section] / Key=Value text, read and written by
//! the game itself instead of GetPrivateProfileString, so it works the
//! same everywhere. Values keep the original's sections and keys.

use std::path::PathBuf;

#[derive(Default, Debug, Clone)]
pub struct Profile {
    path: Option<PathBuf>,
    /// (section, [(key, value)]) in file order.
    sections: Vec<(String, Vec<(String, String)>)>,
    section: String,
}

impl Profile {
    /// Profile::Open: uses `path` (created on the first write).
    pub fn open(path: PathBuf) -> Self {
        let mut p = Profile { path: Some(path), ..Default::default() };
        if let Some(text) = p.path.as_ref().and_then(|f| std::fs::read(f).ok()) {
            p.parse(&String::from_utf8_lossy(&text));
        }
        p
    }

    fn parse(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                self.sections.push((name.trim().to_string(), Vec::new()));
            } else if let Some((k, v)) = line.split_once('=') {
                if self.sections.is_empty() {
                    self.sections.push((String::new(), Vec::new()));
                }
                self.sections.last_mut().unwrap().1.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let mut out = String::new();
        for (name, entries) in &self.sections {
            out.push_str(&format!("[{name}]\n"));
            for (k, v) in entries {
                out.push_str(&format!("{k}={v}\n"));
            }
        }
        if let Err(e) = std::fs::write(path, out) {
            crate::log::warn(&format!("cannot write {}: {e}", path.display()));
        }
    }

    /// Selects the [section] later reads and writes use.
    pub fn set_section(&mut self, section: &str) {
        self.section = section.to_string();
    }

    /// GetPrivateProfileString: section and key names ignore case.
    fn get(&self, key: &str) -> Option<&str> {
        if self.path.is_none() || self.section.is_empty() {
            return None;
        }
        self.sections
            .iter()
            .filter(|(s, _)| s.eq_ignore_ascii_case(&self.section))
            .flat_map(|(_, e)| e.iter())
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    pub fn get_string(&self, key: &str, def: &str) -> String {
        self.get(key).unwrap_or(def).to_string()
    }

    /// GetInt: any base strtol takes (0x.., 0..), else `def`.
    pub fn get_int(&self, key: &str, def: i32) -> i32 {
        match self.get(key) {
            Some(v) if !v.is_empty() => strtol(v),
            _ => def,
        }
    }

    /// GetBool: anything but "0" is set.
    pub fn get_bool(&self, key: &str, def: bool) -> bool {
        match self.get(key) {
            Some(v) if !v.is_empty() => !v.starts_with('0'),
            _ => def,
        }
    }

    /// WritePrivateProfileString (written through at once).
    pub fn write_string(&mut self, key: &str, value: &str) {
        if self.path.is_none() || self.section.is_empty() {
            return;
        }
        let section = match self.sections.iter().position(|(s, _)| s.eq_ignore_ascii_case(&self.section)) {
            Some(i) => i,
            None => {
                self.sections.push((self.section.clone(), Vec::new()));
                self.sections.len() - 1
            }
        };
        let entries = &mut self.sections[section].1;
        match entries.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
            Some(e) => e.1 = value.to_string(),
            None => entries.push((key.to_string(), value.to_string())),
        }
        self.save();
    }

    pub fn write_int(&mut self, key: &str, value: i32) {
        self.write_string(key, &value.to_string());
    }

    pub fn write_bool(&mut self, key: &str, value: bool) {
        self.write_string(key, if value { "1" } else { "0" });
    }
}

/// strtol(s, NULL, 0): leading sign, 0x hex, 0 octal, stops at junk.
fn strtol(s: &str) -> i32 {
    let s = s.trim_start();
    let (neg, s) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let (radix, digits) = if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        (16, h)
    } else if s.len() > 1 && s.starts_with('0') {
        (8, &s[1..])
    } else {
        (10, s)
    };
    let mut v: i64 = 0;
    for c in digits.chars() {
        let Some(d) = c.to_digit(radix) else { break };
        v = (v * radix as i64 + d as i64).min(i64::from(u32::MAX) + 1);
    }
    let v = if neg { -v } else { v };
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_sections() {
        let dir = std::env::temp_dir().join(format!("wf-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wf.ini");
        std::fs::write(&path, "[Option]\nBGMVolume=3\nRecAllStage=1\n[Other]\nX=0x10\n").unwrap();
        let mut p = Profile::open(path.clone());
        p.set_section("option");
        assert_eq!(p.get_int("bgmvolume", 8), 3);
        assert_eq!(p.get_int("SEVolume", 8), 8);
        assert!(p.get_bool("RecAllStage", false));
        p.set_section("Other");
        assert_eq!(p.get_int("X", 0), 16);
        p.set_section("Option");
        p.write_int("SEVolume", 5);
        let again = Profile::open(path);
        let mut again = again;
        again.set_section("Option");
        assert_eq!(again.get_int("SEVolume", 8), 5);
        assert_eq!(again.get_int("BGMVolume", 8), 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn strtol_bases() {
        assert_eq!(strtol("12abc"), 12);
        assert_eq!(strtol("-7"), -7);
        assert_eq!(strtol("010"), 8);
        assert_eq!(strtol("0x1F"), 31);
    }
}
