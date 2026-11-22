//! Config: `section.key = value` settings in a small INI file (wf-plus.ini
//! next to the game's own wf.ini), overridden from the command line.
//! Unknown keys are kept, so a file round-trips.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::time::FpsCap;

#[derive(Clone, Debug, Default)]
pub struct Config {
    /// "section.key" -> value.
    values: BTreeMap<String, String>,
    path: Option<PathBuf>,
}

impl Config {
    /// Reads `path` (an empty config when it does not exist).
    pub fn load(path: &Path) -> Self {
        let mut c = Config { values: BTreeMap::new(), path: Some(path.to_path_buf()) };
        if let Ok(text) = crate::fs::read_to_string(path) {
            c.parse(&text);
        }
        c
    }

    pub fn parse(&mut self, text: &str) {
        let mut section = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = s.trim().to_ascii_lowercase();
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let key = if section.is_empty() { k.trim().to_ascii_lowercase() } else { format!("{section}.{}", k.trim().to_ascii_lowercase()) };
                self.values.insert(key, v.trim().to_string());
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn get_or<T: std::str::FromStr>(&self, key: &str, default: T) -> T {
        self.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        match self.get(key).map(|v| v.to_ascii_lowercase()) {
            Some(v) if ["1", "on", "true", "yes"].contains(&v.as_str()) => true,
            Some(v) if ["0", "off", "false", "no"].contains(&v.as_str()) => false,
            _ => default,
        }
    }

    pub fn set(&mut self, key: &str, value: impl ToString) {
        self.values.insert(key.to_ascii_lowercase(), value.to_string());
    }

    /// The keys under `section.` (without the prefix), in order.
    pub fn section(&self, section: &str) -> Vec<(String, String)> {
        let prefix = format!("{section}.");
        self.values.iter().filter_map(|(k, v)| k.strip_prefix(&prefix).map(|k| (k.to_string(), v.clone()))).collect()
    }

    /// Writes the config back to its file.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        let mut out = String::from("# Warning Forever (Rust port) engine settings.\n");
        let mut section = String::new();
        for (k, v) in &self.values {
            let (s, key) = k.split_once('.').unwrap_or(("", k));
            if s != section {
                out.push_str(&format!("\n[{s}]\n"));
                section = s.to_string();
            }
            out.push_str(&format!("{key} = {v}\n"));
        }
        crate::fs::write(path, out)
    }
}

/// How the window is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMode {
    Windowed,
    /// Exclusive fullscreen at the desktop mode.
    Fullscreen,
    /// A borderless window covering the display.
    Borderless,
}

/// The display settings (section `display`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayConfig {
    pub width: u32,
    pub height: u32,
    pub mode: WindowMode,
    pub fps_cap: FpsCap,
    /// Render target pixels per window pixel (1 = native, DPI-aware).
    pub render_scale: f32,
    pub show_fps: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        // Android: the whole screen, at the display's rate.
        #[cfg(target_os = "android")]
        let (mode, fps_cap) = (WindowMode::Fullscreen, FpsCap::VSync);
        #[cfg(not(target_os = "android"))]
        let (mode, fps_cap) = (WindowMode::Windowed, FpsCap::Off);
        DisplayConfig { width: 1280, height: 720, mode, fps_cap, render_scale: 1.0, show_fps: false }
    }
}

impl DisplayConfig {
    pub fn from_config(c: &Config) -> Self {
        let d = DisplayConfig::default();
        DisplayConfig {
            width: c.get_or("display.width", d.width).max(64),
            height: c.get_or("display.height", d.height).max(64),
            mode: match c.get("display.mode").map(|m| m.to_ascii_lowercase()) {
                Some(m) if m == "fullscreen" => WindowMode::Fullscreen,
                Some(m) if m == "borderless" => WindowMode::Borderless,
                _ => WindowMode::Windowed,
            },
            fps_cap: c.get("display.fps_cap").and_then(FpsCap::parse).unwrap_or(d.fps_cap),
            render_scale: c.get_or("display.render_scale", d.render_scale).clamp(0.25, 4.0),
            show_fps: c.get_bool("display.show_fps", d.show_fps),
        }
    }

    pub fn store(&self, c: &mut Config) {
        c.set("display.width", self.width);
        c.set("display.height", self.height);
        c.set(
            "display.mode",
            match self.mode {
                WindowMode::Windowed => "windowed",
                WindowMode::Fullscreen => "fullscreen",
                WindowMode::Borderless => "borderless",
            },
        );
        c.set(
            "display.fps_cap",
            match self.fps_cap {
                FpsCap::Off => "off".to_string(),
                FpsCap::VSync => "vsync".to_string(),
                FpsCap::Fps(f) => format!("{f}"),
            },
        );
        c.set("display.render_scale", self.render_scale);
        c.set("display.show_fps", self.show_fps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_and_types() {
        let mut c = Config::default();
        c.parse("[Display]\nwidth = 1920\nfps_cap = 144\nmode=borderless\n[input]\nbind.shot = key:z, pad:south\n");
        let d = DisplayConfig::from_config(&c);
        assert_eq!((d.width, d.height), (1920, 720));
        assert_eq!(d.fps_cap, FpsCap::Fps(144.0));
        assert_eq!(d.mode, WindowMode::Borderless);
        assert_eq!(c.section("input"), vec![("bind.shot".to_string(), "key:z, pad:south".to_string())]);
    }
}
