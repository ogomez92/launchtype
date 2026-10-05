//! User settings, byte-compatible with the Python app's `settings.json`
//! (`managers/settings_manager.py`). Only known keys are loaded or saved;
//! unknown keys in the file are dropped, exactly like the Python whitelist.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage::atomic_write_json;

/// Written with [`crate::portable`] placeholders so the default survives a
/// move to another machine. The macOS and Linux defaults used to carry a
/// literal `~`, which nothing ever expanded.
#[cfg(windows)]
pub const DEFAULT_STEAM_LIBRARY: &str = r"{{programfiles86}}\Steam\steamapps";
#[cfg(target_os = "macos")]
pub const DEFAULT_STEAM_LIBRARY: &str = "{{appdata}}/Steam/steamapps";
#[cfg(not(any(windows, target_os = "macos")))]
pub const DEFAULT_STEAM_LIBRARY: &str = "{{home}}/.steam/steam/steamapps";

pub const DEFAULT_AI_MODEL: &str = "claude-opus-4-8";

/// `language` value meaning "follow the operating system locale".
pub const LANGUAGE_SYSTEM: &str = "system";

pub const DEFAULT_SSH_PORT: u16 = 22;

/// Long enough to look several secrets up in one sitting, short enough that
/// walking away from the machine closes the vault behind you.
pub const DEFAULT_VAULT_LOCK_MINUTES: u32 = 5;

/// Long enough to switch windows and paste, short enough that a password is
/// not still sitting on the clipboard an hour later.
pub const DEFAULT_VAULT_CLIPBOARD_SECONDS: u32 = 30;

/// Whisper model used by path mode's transcription. For the OpenAI-style CLIs
/// this is a model *name* they download for themselves; for whisper.cpp it has
/// to be the path of a `ggml-*.bin` file, which is why it is free text rather
/// than a dropdown.
pub const DEFAULT_WHISPER_MODEL: &str = "base";

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
    /// UI language: `"system"` follows the OS locale, otherwise a catalog code
    /// such as `"en"` or `"es"`. Applied at startup.
    pub language: String,
    /// Active commands file, relative to the app folder. Switchable from
    /// Settings; the `-c` command line flag overrides it for the current run.
    pub commands_file: String,
    /// SSH mode ($) target. The key is preferred over the password; when both
    /// are set the password is also tried as the key's passphrase.
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub ssh_key_path: String,
    pub ssh_password: String,
    /// Whether to check for machine-specific paths at startup and offer to
    /// replace them with placeholders. Turned off by "Never ask again".
    pub portability_check: bool,
    /// Minutes the encrypted vault (`*`) stays unlocked without being used
    /// before its key is wiped from memory. 0 additionally re-locks it the
    /// moment a secret has been copied, so the master password is asked for
    /// every single time.
    pub vault_lock_minutes: u32,
    /// Seconds after which a copied vault secret is taken back off the
    /// clipboard, provided nothing else has been copied since. 0 never clears.
    pub vault_clipboard_seconds: u32,
    /// Path mode (`/`) conversions: where ffmpeg lives. Empty means "look on
    /// PATH and in the usual install folders". Either binary of the pair, or
    /// the folder holding them, is accepted.
    pub ffmpeg_path: String,
    /// Whether a conversion deletes the file it converted, once ffprobe has
    /// confirmed the new one really is that recording. Off keeps both.
    pub delete_original_after_convert: bool,
    /// Path mode transcription: the speech recogniser to run. Empty looks for
    /// `whisper-cli`, `whisper` and the rest on PATH. Claude itself has no
    /// audio input, which is why this is a separate program.
    pub whisper_path: String,
    /// The model that recogniser should use: a name for the OpenAI-style CLIs,
    /// a `ggml-*.bin` file for whisper.cpp.
    pub whisper_model: String,
}

pub const DEFAULT_COMMANDS_FILE: &str = "commands.json";

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
            language: LANGUAGE_SYSTEM.to_string(),
            commands_file: DEFAULT_COMMANDS_FILE.to_string(),
            ssh_host: String::new(),
            ssh_port: DEFAULT_SSH_PORT,
            ssh_user: String::new(),
            ssh_key_path: String::new(),
            ssh_password: String::new(),
            portability_check: true,
            vault_lock_minutes: DEFAULT_VAULT_LOCK_MINUTES,
            vault_clipboard_seconds: DEFAULT_VAULT_CLIPBOARD_SECONDS,
            ffmpeg_path: String::new(),
            delete_original_after_convert: true,
            whisper_path: String::new(),
            whisper_model: DEFAULT_WHISPER_MODEL.to_string(),
        }
    }
}

impl Settings {
    /// True when SSH mode has enough configuration to attempt a connection.
    pub fn ssh_configured(&self) -> bool {
        !self.ssh_host.trim().is_empty()
            && !self.ssh_user.trim().is_empty()
            && (!self.ssh_key_path.trim().is_empty() || !self.ssh_password.is_empty())
    }

    /// Settings values that name a file or folder, and so break when the app
    /// moves to another machine. The portability scan covers these alongside
    /// the commands.
    pub fn machine_specific_paths(&self) -> Vec<String> {
        [
            self.steam_library.clone(),
            self.ssh_key_path.clone(),
            self.ffmpeg_path.clone(),
            self.whisper_path.clone(),
        ]
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .collect()
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
    fn ssh_needs_a_host_a_user_and_one_credential() {
        let mut settings = Settings::default();
        assert!(!settings.ssh_configured());
        settings.ssh_host = "example.com".into();
        settings.ssh_user = "me".into();
        assert!(!settings.ssh_configured(), "no key and no password");
        settings.ssh_password = "hunter2".into();
        assert!(settings.ssh_configured());
        settings.ssh_password.clear();
        settings.ssh_key_path = "id_ed25519".into();
        assert!(settings.ssh_configured(), "a key alone is enough");
        settings.ssh_user = "   ".into();
        assert!(!settings.ssh_configured(), "a blank user is no user");
    }

    /// The defaults must not carry a literal `~` or a hardcoded user folder:
    /// nothing expands those, and the app is meant to move between machines.
    #[test]
    fn the_default_steam_library_is_machine_independent() {
        let default = Settings::default().steam_library;
        assert!(!default.contains('~'), "unexpandable home shortcut in {default}");
        assert!(
            crate::portable::placeholder_names(&default).next().is_some(),
            "{default} has no placeholder"
        );
    }

    #[test]
    fn machine_specific_paths_skips_blanks() {
        let mut settings = Settings { ssh_key_path: "  ".into(), ..Default::default() };
        assert_eq!(settings.machine_specific_paths(), vec![settings.steam_library.clone()]);
        settings.ssh_key_path = r"C:\Users\me\.ssh\id".into();
        assert_eq!(settings.machine_specific_paths().len(), 2);
    }

    /// A settings.json written before path mode must not read as "never delete
    /// the original": the default is the one the mode was asked for.
    #[test]
    fn a_file_from_before_path_mode_gets_the_path_mode_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"enable_sounds": true}"#).unwrap();
        let store = SettingsStore::load(&path);
        assert!(store.settings.delete_original_after_convert);
        assert_eq!(store.settings.whisper_model, DEFAULT_WHISPER_MODEL);
        assert!(store.settings.ffmpeg_path.is_empty(), "empty means look it up");
    }

    #[test]
    fn the_portability_check_is_on_until_dismissed() {
        assert!(Settings::default().portability_check);
    }

    /// A settings.json written before the vault existed must not read as
    /// "never lock" / "never clear the clipboard".
    #[test]
    fn a_file_from_before_the_vault_gets_the_vault_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"enable_sounds": true}"#).unwrap();
        let store = SettingsStore::load(&path);
        assert_eq!(store.settings.vault_lock_minutes, DEFAULT_VAULT_LOCK_MINUTES);
        assert_eq!(store.settings.vault_clipboard_seconds, DEFAULT_VAULT_CLIPBOARD_SECONDS);
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
