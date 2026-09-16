//! Startup defaults, stored as key=value lines next to the bookmarks.

use std::path::PathBuf;

pub struct Settings {
    pub dark: bool,
    pub fit: i32,
    pub spread: bool,
    pub rtl: bool,
    pub rail: bool,
    pub strip: bool,
    pub resume: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            dark: true,
            fit: 0,
            spread: false,
            rtl: false,
            rail: true,
            strip: false,
            resume: true,
        }
    }
}

pub fn data_dir() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("APPDATA") {
        return Some(PathBuf::from(v)); // Windows
    }
    if let Some(v) = std::env::var_os("XDG_DATA_HOME") {
        return Some(PathBuf::from(v));
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    Some(if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        home.join(".local/share")
    })
}

fn path() -> Option<PathBuf> {
    let dir = data_dir()?.join("mangix");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("settings.conf"))
}

pub fn load() -> Settings {
    let mut s = Settings::default();
    let Some(file) = path() else { return s };
    let Ok(text) = std::fs::read_to_string(file) else {
        return s;
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let on = value.trim() == "true";
        match key.trim() {
            "dark" => s.dark = on,
            "fit" => s.fit = value.trim().parse().unwrap_or(0).clamp(0, 3),
            "spread" => s.spread = on,
            "rtl" => s.rtl = on,
            "rail" => s.rail = on,
            "strip" => s.strip = on,
            "resume" => s.resume = on,
            _ => {}
        }
    }
    s
}

pub fn save(s: &Settings) {
    let Some(file) = path() else { return };
    let text = format!(
        "dark={}\nfit={}\nspread={}\nrtl={}\nrail={}\nresume={}\nstrip={}\n",
        s.dark, s.fit, s.spread, s.rtl, s.rail, s.resume, s.strip
    );
    let _ = std::fs::write(file, text);
}
