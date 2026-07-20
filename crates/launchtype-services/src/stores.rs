//! Persistent wrappers around the pure timer/alarm engines: they replicate
//! exactly when the Python services rewrite their JSON files (adds/removes
//! always; alarm toggles too because `enabled` persists; timer toggles never —
//! live countdowns are memory-only).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local};
use launchtype_core::alarms::{load_alarm_defs, AlarmDef, AlarmEngine};
use launchtype_core::storage::atomic_write_json;
use launchtype_core::timers::{load_timer_defs, TimerDef, TimerEngine};

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
