use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub wpm: f32,
    pub target_seconds: f32,
    pub use_target_time: bool,
    pub speed_variation: f32,
    pub min_delay_ms: u64,
    pub max_delay_ms: u64,
    pub burst_probability: f32,
    pub mistakes_enabled: bool,
    pub mistake_rate: f32,
    pub thinking_enabled: bool,
    pub thinking_probability: f32,
    pub thinking_min_ms: u64,
    pub thinking_max_ms: u64,
    pub punctuation_pause_ms: u64,
    pub revision_enabled: bool,
    pub revision_probability: f32,
    pub revision_min_seconds: u64,
    pub revision_max_seconds: u64,
    pub max_revisions: u32,
    pub revision_aggressiveness: f32,
    pub countdown_seconds: u64,
    pub hotkey_start: u32,
    pub hotkey_pause: u32,
    pub hotkey_stop: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            wpm: 62.0,
            target_seconds: 90.0,
            use_target_time: false,
            speed_variation: 0.18,
            min_delay_ms: 18,
            max_delay_ms: 420,
            burst_probability: 0.08,
            mistakes_enabled: true,
            mistake_rate: 0.012,
            thinking_enabled: true,
            thinking_probability: 0.035,
            thinking_min_ms: 1000,
            thinking_max_ms: 3000,
            punctuation_pause_ms: 420,
            revision_enabled: true,
            revision_probability: 0.12,
            revision_min_seconds: 8,
            revision_max_seconds: 38,
            max_revisions: 2,
            revision_aggressiveness: 0.35,
            countdown_seconds: 3,
            hotkey_start: 117,
            hotkey_pause: 118,
            hotkey_stop: 119,
        }
    }
}

impl Settings {
    fn path() -> Option<PathBuf> {
        ProjectDirs::from("com", "AutoWrite", "Auto Write")
            .map(|dirs| dirs.config_dir().join("settings.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, data);
        }
    }
}
