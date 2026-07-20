//! Time-of-day alarm engine — pure port of `services/alarm_service.py`.
//! Each alarm fires once per day at its hour:minute (24h) while enabled;
//! the once-per-minute guard prevents re-firing within the same minute.

use std::collections::HashMap;

use chrono::{DateTime, Local, Timelike};
use serde::{Deserialize, Serialize};

use crate::i18n::{format_args, tr, Arg};

/// One entry of `alarms.json` (field order matches the Python dict).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlarmDef {
    pub id: String,
    #[serde(rename = "type", default = "alarm_type")]
    pub kind: String,
    pub title: String,
    pub description: String,
    pub hour: u32,
    pub minute: u32,
    #[serde(default)]
    pub sound: Option<String>,
    #[serde(default)]
    pub enabled: bool,
}

fn alarm_type() -> String {
    "alarm".to_string()
}

impl AlarmDef {
    pub fn new(
        title: String,
        description: String,
        hour: u32,
        minute: u32,
        sound: Option<String>,
    ) -> Self {
        AlarmDef {
            id: uuid::Uuid::new_v4().to_string(),
            kind: alarm_type(),
            title,
            description,
            hour,
            minute,
            sound,
            enabled: true,
        }
    }
}

pub struct AlarmEngine {
    pub alarms: Vec<AlarmDef>,
    /// alarm id -> "YYYY-MM-DD HH:MM" guard against re-firing.
    last_fired: HashMap<String, String>,
}

impl AlarmEngine {
    pub fn from_defs(alarms: Vec<AlarmDef>) -> Self {
        AlarmEngine { alarms, last_fired: HashMap::new() }
    }

    pub fn add(&mut self, def: AlarmDef) {
        self.alarms.push(def);
    }

    /// Toggle an alarm's activation state. Returns the new enabled state, or
    /// `None` for an unknown id.
    pub fn toggle(&mut self, alarm_id: &str) -> Option<bool> {
        let alarm = self.alarms.iter_mut().find(|a| a.id == alarm_id)?;
        alarm.enabled = !alarm.enabled;
        Some(alarm.enabled)
    }

    pub fn remove(&mut self, alarm_id: &str) {
        self.alarms.retain(|a| a.id != alarm_id);
        self.last_fired.remove(alarm_id);
    }

    /// Enabled alarms whose hour:minute matches `now` and that have not
    /// already fired this minute.
    pub fn due(&mut self, now: DateTime<Local>) -> Vec<AlarmDef> {
        let key = now.format("%Y-%m-%d %H:%M").to_string();
        let mut fired = Vec::new();
        for alarm in &self.alarms {
            if !alarm.enabled {
                continue;
            }
            if alarm.hour == now.hour() && alarm.minute == now.minute() {
                if self.last_fired.get(&alarm.id) != Some(&key) {
                    self.last_fired.insert(alarm.id.clone(), key.clone());
                    fired.push(alarm.clone());
                }
            }
        }
        fired
    }

    /// The localized "{title} - HH:MM (on/off)" list label for an alarm.
    pub fn item_label(alarm: &AlarmDef) -> String {
        let state = if alarm.enabled { tr("on") } else { tr("off") };
        format_args(
            &tr("{title} - {hour:02d}:{minute:02d} ({state})"),
            &[
                ("title", Arg::Str(&alarm.title)),
                ("hour", Arg::Int(alarm.hour as i64)),
                ("minute", Arg::Int(alarm.minute as i64)),
                ("state", Arg::Str(&state)),
            ],
        )
    }
}

/// Load alarm definitions; missing or corrupt file yields an empty list.
pub fn load_alarm_defs(path: &std::path::Path) -> Vec<AlarmDef> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32, s: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 7, 20, h, m, s).unwrap()
    }

    fn alarm(h: u32, m: u32) -> AlarmDef {
        AlarmDef::new("wake".into(), "get up".into(), h, m, None)
    }

    #[test]
    fn fires_once_per_minute_while_enabled() {
        let mut engine = AlarmEngine::from_defs(vec![alarm(7, 30)]);
        assert!(engine.due(at(7, 29, 59)).is_empty());
        assert_eq!(engine.due(at(7, 30, 0)).len(), 1);
        // Same minute, checked again 20s later: guarded.
        assert!(engine.due(at(7, 30, 20)).is_empty());
        assert!(engine.due(at(7, 31, 0)).is_empty());
        // Next day, same minute: fires again (different date key).
        let next_day = Local.with_ymd_and_hms(2026, 7, 21, 7, 30, 5).unwrap();
        assert_eq!(engine.due(next_day).len(), 1);
    }

    #[test]
    fn disabled_alarms_are_skipped() {
        let mut a = alarm(7, 30);
        a.enabled = false;
        let mut engine = AlarmEngine::from_defs(vec![a]);
        assert!(engine.due(at(7, 30, 0)).is_empty());

        let id = engine.alarms[0].id.clone();
        assert_eq!(engine.toggle(&id), Some(true));
        assert_eq!(engine.due(at(7, 30, 30)).len(), 1);
        assert_eq!(engine.toggle(&id), Some(false));
        assert_eq!(engine.toggle("nope"), None);
    }

    #[test]
    fn json_round_trip_matches_python_shape() {
        let json = r#"[{"id": "a1", "type": "alarm", "title": "wake", "description": "get up", "hour": 7, "minute": 30, "sound": null, "enabled": true}]"#;
        let defs: Vec<AlarmDef> = serde_json::from_str(json).unwrap();
        assert!(defs[0].enabled);
        let out = crate::storage::to_python_json(&defs, None).unwrap();
        assert_eq!(out, json);
    }

    #[test]
    fn item_label_english() {
        let mut a = alarm(7, 5);
        assert_eq!(AlarmEngine::item_label(&a), "wake - 07:05 (on)");
        a.enabled = false;
        assert_eq!(AlarmEngine::item_label(&a), "wake - 07:05 (off)");
    }
}
