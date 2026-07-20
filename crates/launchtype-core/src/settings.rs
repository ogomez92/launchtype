//! User settings, byte-compatible with the Python app's `settings.json`
//! (`managers/settings_manager.py`). Only known keys are loaded or saved;
//! unknown keys in the file are dropped, exactly like the Python whitelist.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage::atomic_write_json;

#[cfg(windows)]
pub const DEFAULT_STEAM_LIBRARY: &str = r"C:\Program Files (x86)\Steam\steamapps";
#[cfg(target_os = "macos")]
pub const DEFAULT_STEAM_LIBRARY: &str = "~/Library/Application Support/Steam/steamapps";
#[cfg(not(any(windows, target_os = "macos")))]
pub const DEFAULT_STEAM_LIBRARY: &str = "~/.steam/steam/steamapps";

pub const DEFAULT_AI_MODEL: &str = "claude-opus-4-8";

/// Field order mirrors the Python DEFAULTS dict so the saved file keeps the
/// same key order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub enable_sounds: bool,
    pub start_minimized: bool,
    pub snippets_on_invoke: bool,
    pub steam_library: String,
    /// Notebrook credentials. Stored locally only, never committed.
    pub notebrook_url: String,
    pub notebrook_token: String,
    /// Claude model used for the AI screenshot description / region features.
    pub ai_model: String,
    /// Commands mode sort order: false = last modified (default), true = by uses.
    pub command_sort_by_uses: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enable_sounds: true,
            start_minimized: false,
            snippets_on_invoke: false,
            steam_library: DEFAULT_STEAM_LIBRARY.to_string(),
            notebrook_url: String::new(),
            notebrook_token: String::new(),
            ai_model: DEFAULT_AI_MODEL.to_string(),
            command_sort_by_uses: false,
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
    pub settings: Settings,
}

impl SettingsStore {
    /// Load settings from `path`; a missing or corrupt file yields defaults
    /// (the Python manager swallows load errors the same way).
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let settings = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        SettingsStore { path, settings }
    }

    pub fn save(&self) -> std::io::Result<()> {
        atomic_write_json(&self.path, &self.settings, Some(2))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::load(dir.path().join("settings.json"));
        assert_eq!(store.settings, Settings::default());
        assert!(store.settings.enable_sounds);
        assert_eq!(store.settings.ai_model, DEFAULT_AI_MODEL);
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{not json").unwrap();
        let store = SettingsStore::load(&path);
        assert_eq!(store.settings, Settings::default());
    }

    #[test]
    fn partial_file_keeps_defaults_for_missing_keys_and_drops_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"enable_sounds": false, "notebrook_token": "tok", "mystery_key": 42}"#,
        )
        .unwrap();
        let store = SettingsStore::load(&path);
        assert!(!store.settings.enable_sounds);
        assert_eq!(store.settings.notebrook_token, "tok");
        assert!(!store.settings.start_minimized);

        store.save().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("mystery_key"));
        assert!(text.contains("\"enable_sounds\": false"));
    }

    #[test]
    fn save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut store = SettingsStore::load(&path);
        store.settings.command_sort_by_uses = true;
        store.settings.steam_library = r"D:\SteamLibrary\steamapps".into();
        store.save().unwrap();

        let reloaded = SettingsStore::load(&path);
        assert_eq!(reloaded.settings, store.settings);
    }
}
