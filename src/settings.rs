//! Terminal settings persistence and font enumeration. No UI-framework deps.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_TERMINAL_FONT: &str = "Cascadia Mono, Cascadia Code, Consolas, monospace";
pub const DEFAULT_TERMINAL_FONT_SIZE: u16 = 14;
pub const MIN_TERMINAL_FONT_SIZE: u16 = 10;
pub const MAX_TERMINAL_FONT_SIZE: u16 = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalSettings {
    #[serde(default = "default_terminal_font")]
    pub font_family: String,
    #[serde(default = "default_terminal_font_size")]
    pub font_size: u16,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            font_family: default_terminal_font(),
            font_size: default_terminal_font_size(),
        }
    }
}

pub fn default_terminal_font() -> String {
    DEFAULT_TERMINAL_FONT.to_owned()
}

pub fn default_terminal_font_size() -> u16 {
    DEFAULT_TERMINAL_FONT_SIZE
}

pub fn normalize_font_family(font_family: String) -> String {
    let font_family = font_family.trim();
    if font_family.is_empty() {
        DEFAULT_TERMINAL_FONT.to_owned()
    } else {
        font_family.to_owned()
    }
}

pub fn normalize_font_size(font_size: u16) -> u16 {
    font_size.clamp(MIN_TERMINAL_FONT_SIZE, MAX_TERMINAL_FONT_SIZE)
}

pub fn load_terminal_settings() -> TerminalSettings {
    let Some(path) = settings_path() else {
        return TerminalSettings::default();
    };
    let Ok(contents) = fs::read_to_string(path) else {
        return TerminalSettings::default();
    };
    let Ok(mut settings) = serde_json::from_str::<TerminalSettings>(&contents) else {
        return TerminalSettings::default();
    };
    settings.font_family = normalize_font_family(settings.font_family);
    settings.font_size = normalize_font_size(settings.font_size);
    settings
}

pub fn save_terminal_settings(settings: &TerminalSettings) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    if let Ok(contents) = serde_json::to_string_pretty(settings) {
        fs::write(path, contents).ok();
    }
}

pub fn settings_path() -> Option<PathBuf> {
    if cfg!(windows) {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("VibeTerm").join("settings.json"))
    } else {
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|path| path.join(".config").join("vibeterm").join("settings.json"))
    }
}

pub fn system_font_families() -> Vec<String> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();

    let mut families = BTreeSet::new();
    for face in database.faces() {
        for (family, _) in &face.families {
            let family = family.trim();
            if !family.is_empty() {
                families.insert(family.to_owned());
            }
        }
    }

    families.into_iter().collect()
}

pub fn default_font_families() -> Vec<String> {
    [
        "Cascadia Mono",
        "Cascadia Code",
        "Consolas",
        "JetBrains Mono",
        "Fira Code",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}
