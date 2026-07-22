//! Data models byte-compatible with the Python app's JSON files.
//!
//! `Option` fields + `flatten` keep round-trips faithful: keys absent in a
//! legacy record stay absent when the file is rewritten, and unknown keys
//! (e.g. `"type"`) survive untouched in `extra`.

use serde::{Deserialize, Serialize};

/// One entry of `commands.json` `"commands"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Command {
    pub path: String,
    /// Display name, stored lowercase for matching.
    pub name: String,
    /// Comma-separated argument string (NOT a list), as typed in the dialog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,
    /// Lowercase shortcut; exact match takes priority over fuzzy search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_as_admin: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_count: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Command {
    pub fn args(&self) -> &str {
        self.args.as_deref().unwrap_or("")
    }

    pub fn shortcut(&self) -> &str {
        self.shortcut.as_deref().unwrap_or("")
    }

    pub fn run_as_admin(&self) -> bool {
        self.run_as_admin.unwrap_or(false)
    }

    pub fn run_count(&self) -> u64 {
        self.run_count.unwrap_or(0)
    }
}

/// The whole `commands.json` document.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CommandsFile {
    pub commands: Vec<Command>,
    /// Lifetime total of command runs; survives command deletions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_runs: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl CommandsFile {
    /// Increment a command's run_count and the lifetime total (mirrors
    /// `DataManager.record_command_run`).
    pub fn record_run(&mut self, command_id: &str) {
        if let Some(cmd) = self.commands.iter_mut().find(|c| c.id == command_id) {
            cmd.run_count = Some(cmd.run_count() + 1);
        }
        self.total_runs = Some(self.total_runs.unwrap_or(0) + 1);
    }
}

/// File names in `dir` that parse as a commands document, sorted
/// alphabetically. Used by Settings to offer the switchable commands files
/// sitting next to the app; `settings.json`, `timers.json` and friends are
/// excluded simply by not having a `"commands"` array.
pub fn commands_files_in(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter(|entry| {
            std::fs::read_to_string(entry.path())
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .and_then(|value| value.get("commands").map(|c| c.is_array()))
                .unwrap_or(false)
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::to_python_json;

    /// A legacy-shaped file: first entry lacks args/shortcut/run_as_admin/
    /// run_count and has no "type"; second is a fully-populated modern entry.
    const SAMPLE: &str = r#"{"commands": [{"path": "C:\\tools\\a.exe", "name": "alpha", "id": "1111"}, {"path": "C:\\tools\\b.exe", "name": "beta", "args": "x, y", "shortcut": "b", "id": "2222", "run_as_admin": true, "run_count": 3, "type": "command"}], "total_runs": 17}"#;

    #[test]
    fn legacy_records_round_trip_byte_identical() {
        let file: CommandsFile = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(file.commands.len(), 2);
        assert_eq!(file.commands[0].run_count(), 0);
        assert!(!file.commands[0].run_as_admin());
        assert_eq!(file.commands[1].args(), "x, y");
        assert_eq!(file.commands[1].extra.get("type").unwrap(), "command");
        assert_eq!(file.total_runs, Some(17));

        let out = to_python_json(&file, None).unwrap();
        assert_eq!(out, SAMPLE);
    }

    #[test]
    fn record_run_increments_count_and_total() {
        let mut file: CommandsFile = serde_json::from_str(SAMPLE).unwrap();
        file.record_run("1111");
        assert_eq!(file.commands[0].run_count(), 1);
        assert_eq!(file.total_runs, Some(18));

        // Unknown id still counts toward the lifetime total (Python increments
        // total_runs unconditionally).
        file.record_run("nope");
        assert_eq!(file.total_runs, Some(19));
    }

    #[test]
    fn commands_files_are_detected_by_shape_not_by_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("work.json"), SAMPLE).unwrap();
        std::fs::write(dir.path().join("commands.json"), r#"{"commands": []}"#).unwrap();
        std::fs::write(dir.path().join("settings.json"), r#"{"enable_sounds": true}"#).unwrap();
        std::fs::write(dir.path().join("timers.json"), "[]").unwrap();
        std::fs::write(dir.path().join("broken.json"), "{not json").unwrap();
        std::fs::write(dir.path().join("notes.txt"), SAMPLE).unwrap();

        assert_eq!(commands_files_in(dir.path()), vec!["commands.json", "work.json"]);
    }
}
