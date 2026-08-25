//! Persistent wrappers around the pure timer/alarm engines: they replicate
//! exactly when the Python services rewrite their JSON files (adds/removes
//! always; alarm toggles too because `enabled` persists; timer toggles never —
//! live countdowns are memory-only).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local};
use launchtype_core::alarms::{load_alarm_defs, AlarmDef, AlarmEngine};
use launchtype_core::model::{Command, CommandsFile};
use launchtype_core::storage::atomic_write_json;
use launchtype_core::timers::{load_timer_defs, TimerDef, TimerEngine};

pub struct CommandsStore {
    path: PathBuf,
    pub file: CommandsFile,
}

impl CommandsStore {
    /// Load commands.json; a corrupt or malformed file is moved aside to
    /// `<path>.corrupt` and replaced with an empty list, so the app always
    /// starts and the data stays recoverable (mirrors loadCommandsFromFile).
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let mut store = CommandsStore { path, file: CommandsFile::default() };
        if !store.path.exists() {
            store.sync();
            return store;
        }
        match std::fs::read_to_string(&store.path)
            .ok()
            .and_then(|text| serde_json::from_str::<CommandsFile>(&text).ok())
        {
            Some(file) => store.file = file,
            None => {
                let mut corrupt = store.path.as_os_str().to_owned();
                corrupt.push(".corrupt");
                let _ = std::fs::rename(&store.path, &corrupt);
                store.sync();
            }
        }
        store
    }

    pub fn sync(&self) {
        let _ = atomic_write_json(&self.path, &self.file, None);
    }

    /// Add a command and persist. The shortcut is lowercased, like the Python
    /// dialog path — `shortcut_exists` and the dialog's duplicate check both
    /// compare it raw against a lowercased input. The display name is stored
    /// as typed: nothing matches on it without folding case first.
    /// Returns the stored command.
    pub fn add_command(
        &mut self,
        path: &str,
        name: &str,
        args: &str,
        shortcut: &str,
        run_as_admin: bool,
        run_count: u64,
    ) -> Command {
        let command = Command {
            path: path.to_string(),
            name: name.to_string(),
            args: Some(args.to_string()),
            shortcut: Some(shortcut.to_lowercase()),
            id: uuid::Uuid::new_v4().to_string(),
            run_as_admin: Some(run_as_admin),
            run_count: Some(run_count),
            extra: Default::default(),
        };
        self.file.commands.push(command.clone());
        self.sync();
        command
    }

    /// Remove the command with `id`; returns whether anything was removed.
    pub fn pop_by_uuid(&mut self, id: &str) -> bool {
        let before = self.file.commands.len();
        self.file.commands.retain(|c| c.id != id);
        let removed = self.file.commands.len() != before;
        if removed {
            self.sync();
        }
        removed
    }

    pub fn record_run(&mut self, id: &str) {
        self.file.record_run(id);
        self.sync();
    }

    /// Append commands imported from another file and persist. Purely
    /// additive; see [`launchtype_core::merge::merge`]. Returns how many
    /// were added.
    pub fn merge_commands(&mut self, selected: &[Command]) -> usize {
        let added = launchtype_core::merge::merge(&mut self.file, selected);
        if added > 0 {
            self.sync();
        }
        added
    }

    pub fn shortcut_exists(&self, shortcut: &str) -> bool {
        !shortcut.is_empty() && self.file.commands.iter().any(|c| c.shortcut() == shortcut)
    }

    /// Commands in display order for an empty search: file order ("last
    /// modified"), or most-used-first (stable for ties) when sorting by uses.
    pub fn display_order(&self, sort_by_uses: bool) -> Vec<Command> {
        let mut commands = self.file.commands.clone();
        if sort_by_uses {
            commands.sort_by_key(|command| std::cmp::Reverse(command.run_count()));
        }
        commands
    }
}

pub struct TimerStore {
    path: PathBuf,
    pub engine: Arc<Mutex<TimerEngine>>,
}

impl TimerStore {
    pub fn load(path: impl Into<PathBuf>, now: DateTime<Local>) -> Self {
        let path = path.into();
        let defs = load_timer_defs(&path);
        let store = TimerStore { path, engine: Arc::new(Mutex::new(TimerEngine::from_defs(defs, now))) };
        // Python writes the file on first run so it exists from the start.
        if !store.path.exists() {
            store.sync();
        }
        store
    }

    fn sync(&self) {
        let engine = self.engine.lock().unwrap();
        let _ = atomic_write_json(&self.path, &engine.timers, None);
    }

    pub fn add(&self, def: TimerDef, now: DateTime<Local>) {
        self.engine.lock().unwrap().add(def, now);
        self.sync();
    }

    pub fn remove(&self, id: &str) {
        self.engine.lock().unwrap().remove(id);
        self.sync();
    }

    /// Persist an edited definition. The lock is released before `sync` takes
    /// its own.
    pub fn update(&self, def: TimerDef, now: DateTime<Local>) -> bool {
        let updated = self.engine.lock().unwrap().update(def, now);
        if updated {
            self.sync();
        }
        updated
    }

    /// Live countdown state is memory-only: no sync (parity with Python).
    pub fn toggle(&self, id: &str, now: DateTime<Local>) -> Option<bool> {
        self.engine.lock().unwrap().toggle(id, now)
    }
}

pub struct AlarmStore {
    path: PathBuf,
    pub engine: Arc<Mutex<AlarmEngine>>,
}

impl AlarmStore {
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let defs = load_alarm_defs(&path);
        let store = AlarmStore { path, engine: Arc::new(Mutex::new(AlarmEngine::from_defs(defs))) };
        if !store.path.exists() {
            store.sync();
        }
        store
    }

    fn sync(&self) {
        let engine = self.engine.lock().unwrap();
        let _ = atomic_write_json(&self.path, &engine.alarms, None);
    }

    pub fn add(&self, def: AlarmDef) {
        self.engine.lock().unwrap().add(def);
        self.sync();
    }

    pub fn remove(&self, id: &str) {
        self.engine.lock().unwrap().remove(id);
        self.sync();
    }

    /// Persist an edited definition. The lock is released before `sync` takes
    /// its own.
    pub fn update(&self, def: AlarmDef) -> bool {
        let updated = self.engine.lock().unwrap().update(def);
        if updated {
            self.sync();
        }
        updated
    }

    /// The enabled flag persists, so toggling rewrites the file.
    pub fn toggle(&self, id: &str) -> Option<bool> {
        let result = self.engine.lock().unwrap().toggle(id);
        if result.is_some() {
            self.sync();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap()
    }

    #[test]
    fn timer_store_persists_defs_but_not_live_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timers.json");
        let store = TimerStore::load(&path, now());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[]");

        store.add(TimerDef::new("tea".into(), "".into(), 5, false, None), now());
        let id = store.engine.lock().unwrap().timers[0].id.clone();
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("\"tea\""));

        store.toggle(&id, now());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), on_disk, "toggle must not rewrite");

        store.remove(&id);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[]");
    }

    #[test]
    fn merging_persists_the_additions_and_leaves_the_rest_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("commands.json");
        std::fs::write(
            &path,
            r#"{"commands": [{"path": "a.exe", "name": "alpha", "id": "1", "run_count": 5}], "total_runs": 5}"#,
        )
        .unwrap();
        let mut store = CommandsStore::load(&path);

        let added = store.merge_commands(&[Command {
            path: "b.exe".into(),
            name: "beta".into(),
            args: None,
            shortcut: Some("b".into()),
            id: "2".into(),
            run_as_admin: None,
            run_count: None,
            extra: Default::default(),
        }]);

        assert_eq!(added, 1);
        let reloaded = CommandsStore::load(&path);
        assert_eq!(reloaded.file.commands.len(), 2);
        assert_eq!(reloaded.file.commands[0].run_count(), 5, "the existing command is untouched");
        assert_eq!(reloaded.file.commands[1].name, "beta");
        assert_eq!(reloaded.file.total_runs, Some(5));
    }

    #[test]
    fn alarm_store_persists_toggles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("alarms.json");
        let store = AlarmStore::load(&path);
        store.add(AlarmDef::new("wake".into(), "".into(), 7, 30, None));
        let id = store.engine.lock().unwrap().alarms[0].id.clone();
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"enabled\": true"));

        store.toggle(&id);
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"enabled\": false"));
    }
}
