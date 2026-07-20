//! Countdown timer engine — pure port of `services/timer_service.py`.
//! Definitions persist to `timers.json`; live deadlines are in-memory only.
//! The background thread lives in `launchtype-services`; this engine just
//! answers "what fires now?" against an injected clock.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Local};
use serde::{Deserialize, Serialize};

use crate::i18n::{format_args, tr, Arg};

/// One entry of `timers.json` (field order matches the Python dict).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimerDef {
    pub id: String,
    #[serde(rename = "type", default = "timer_type")]
    pub kind: String,
    pub title: String,
    pub description: String,
    pub minutes: u64,
    pub repeating: bool,
    #[serde(default)]
    pub sound: Option<String>,
}

fn timer_type() -> String {
    "timer".to_string()
}

impl TimerDef {
    pub fn new(
        title: String,
        description: String,
        minutes: u64,
        repeating: bool,
        sound: Option<String>,
    ) -> Self {
        TimerDef {
            id: uuid::Uuid::new_v4().to_string(),
            kind: timer_type(),
            title,
            description,
            minutes,
            repeating,
            sound,
        }
    }

    fn period(&self) -> Duration {
        Duration::seconds(self.minutes as i64 * 60)
    }
}

/// Render remaining seconds as `M:SS` (or `H:MM:SS` past an hour).
pub fn format_remaining(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

pub struct TimerEngine {
    pub timers: Vec<TimerDef>,
    next_fire: HashMap<String, Option<DateTime<Local>>>,
}

impl TimerEngine {
    /// Repeating timers default to on whenever they are loaded.
    pub fn from_defs(timers: Vec<TimerDef>, now: DateTime<Local>) -> Self {
        let mut next_fire = HashMap::new();
        for t in &timers {
            if t.repeating {
                next_fire.insert(t.id.clone(), Some(now + t.period()));
            }
        }
        TimerEngine { timers, next_fire }
    }

    /// Repeating timers start on by default.
    pub fn add(&mut self, def: TimerDef, now: DateTime<Local>) {
        if def.repeating {
            self.next_fire.insert(def.id.clone(), Some(now + def.period()));
        }
        self.timers.push(def);
    }

    /// Run/toggle a timer. Returns the new active state, or `None` for an
    /// unknown id. Non-repeating timers (re)start their countdown each run.
    pub fn toggle(&mut self, timer_id: &str, now: DateTime<Local>) -> Option<bool> {
        let timer = self.timers.iter().find(|t| t.id == timer_id)?;
        if timer.repeating && self.is_active(timer_id) {
            self.next_fire.insert(timer_id.to_string(), None);
            return Some(false);
        }
        let deadline = now + timer.period();
        self.next_fire.insert(timer_id.to_string(), Some(deadline));
        Some(true)
    }

    pub fn is_active(&self, timer_id: &str) -> bool {
        matches!(self.next_fire.get(timer_id), Some(Some(_)))
    }

    /// Seconds left until the timer next fires, or `None` when inactive.
    pub fn remaining_seconds(&self, timer_id: &str, now: DateTime<Local>) -> Option<i64> {
        let deadline = (*self.next_fire.get(timer_id)?)?;
        Some(((deadline - now).num_milliseconds() as f64 / 1000.0).round().max(0.0) as i64)
    }

    pub fn remove(&mut self, timer_id: &str) {
        self.timers.retain(|t| t.id != timer_id);
        self.next_fire.remove(timer_id);
    }

    /// Timers whose deadline has passed; repeating ones reschedule from `now`,
    /// one-shot ones deactivate.
    pub fn due(&mut self, now: DateTime<Local>) -> Vec<TimerDef> {
        let mut fired = Vec::new();
        for timer in &self.timers {
            let deadline = match self.next_fire.get(&timer.id) {
                Some(Some(d)) => *d,
                _ => continue,
            };
            if now >= deadline {
                fired.push(timer.clone());
                let next = if timer.repeating { Some(now + timer.period()) } else { None };
                self.next_fire.insert(timer.id.clone(), next);
            }
        }
        fired
    }

    /// The localized "{title} - {descriptor} ({state})" list label for a timer.
    pub fn item_label(&self, timer: &TimerDef, now: DateTime<Local>) -> String {
        let state = match self.remaining_seconds(&timer.id, now) {
            Some(remaining) => {
                let verb = if timer.repeating { tr("until repeat") } else { tr("left") };
                format_args(
                    &tr("running, {time} {verb}"),
                    &[("time", Arg::Str(&format_remaining(remaining))), ("verb", Arg::Str(&verb))],
                )
            }
            None => tr("stopped"),
        };
        let descriptor = if timer.repeating {
            format_args(
                &tr("every {minutes} min, repeating"),
                &[("minutes", Arg::Int(timer.minutes as i64))],
            )
        } else {
            format_args(&tr("{minutes} min"), &[("minutes", Arg::Int(timer.minutes as i64))])
        };
        format_args(
            &tr("{title} - {descriptor} ({state})"),
            &[
                ("title", Arg::Str(&timer.title)),
                ("descriptor", Arg::Str(&descriptor)),
                ("state", Arg::Str(&state)),
            ],
        )
    }
}

/// Load timer definitions; a missing or corrupt file yields an empty list
/// (matching the Python service).
pub fn load_timer_defs(path: &std::path::Path) -> Vec<TimerDef> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t0() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap()
    }

    fn one_shot(minutes: u64) -> TimerDef {
        TimerDef::new("tea".into(), "tea is ready".into(), minutes, false, None)
    }

    fn repeating(minutes: u64) -> TimerDef {
        TimerDef::new("stretch".into(), "stand up".into(), minutes, true, None)
    }

    #[test]
    fn one_shot_fires_once_after_deadline() {
        let timer = one_shot(5);
        let id = timer.id.clone();
        let mut engine = TimerEngine::from_defs(vec![timer], t0());
        assert!(!engine.is_active(&id), "one-shot timers load inactive");

        assert_eq!(engine.toggle(&id, t0()), Some(true));
        assert_eq!(engine.remaining_seconds(&id, t0()), Some(300));

        assert!(engine.due(t0() + Duration::seconds(299)).is_empty());
        let fired = engine.due(t0() + Duration::seconds(300));
        assert_eq!(fired.len(), 1);
        assert!(!engine.is_active(&id), "one-shot deactivates after firing");
        assert!(engine.due(t0() + Duration::seconds(301)).is_empty());
    }

    #[test]
    fn one_shot_retoggle_resets_countdown() {
        let timer = one_shot(5);
        let id = timer.id.clone();
        let mut engine = TimerEngine::from_defs(vec![timer], t0());
        engine.toggle(&id, t0());
        // Re-running while counting down resets, it does not deactivate.
        assert_eq!(engine.toggle(&id, t0() + Duration::seconds(200)), Some(true));
        assert_eq!(
            engine.remaining_seconds(&id, t0() + Duration::seconds(200)),
            Some(300)
        );
    }

    #[test]
    fn repeating_defaults_on_and_reschedules() {
        let timer = repeating(10);
        let id = timer.id.clone();
        let mut engine = TimerEngine::from_defs(vec![timer], t0());
        assert!(engine.is_active(&id), "repeating timers default on when loaded");

        let fired = engine.due(t0() + Duration::seconds(600));
        assert_eq!(fired.len(), 1);
        assert!(engine.is_active(&id), "repeating timer stays active");
        // Rescheduled from fire time, not original deadline.
        assert_eq!(
            engine.remaining_seconds(&id, t0() + Duration::seconds(600)),
            Some(600)
        );
    }

    #[test]
    fn repeating_toggles_on_off() {
        let timer = repeating(10);
        let id = timer.id.clone();
        let mut engine = TimerEngine::from_defs(vec![timer], t0());
        assert_eq!(engine.toggle(&id, t0()), Some(false));
        assert!(!engine.is_active(&id));
        assert_eq!(engine.toggle(&id, t0()), Some(true));
        assert!(engine.is_active(&id));
        assert_eq!(engine.toggle("nope", t0()), None);
    }

    #[test]
    fn json_round_trip_matches_python_shape() {
        let json = r#"[{"id": "abc", "type": "timer", "title": "tea", "description": "ready", "minutes": 3, "repeating": false, "sound": "ding.wav"}]"#;
        let defs: Vec<TimerDef> = serde_json::from_str(json).unwrap();
        assert_eq!(defs[0].minutes, 3);
        let out = crate::storage::to_python_json(&defs, None).unwrap();
        assert_eq!(out, json);
    }

    #[test]
    fn format_remaining_matches_python() {
        assert_eq!(format_remaining(59), "0:59");
        assert_eq!(format_remaining(60), "1:00");
        assert_eq!(format_remaining(3599), "59:59");
        assert_eq!(format_remaining(3600), "1:00:00");
        assert_eq!(format_remaining(3661), "1:01:01");
    }

    #[test]
    fn item_label_english() {
        let timer = one_shot(5);
        let id = timer.id.clone();
        let mut engine = TimerEngine::from_defs(vec![timer], t0());
        let def = engine.timers[0].clone();
        assert_eq!(engine.item_label(&def, t0()), "tea - 5 min (stopped)");
        engine.toggle(&id, t0());
        assert_eq!(
            engine.item_label(&def, t0()),
            "tea - 5 min (running, 5:00 left)"
        );

        let rep = repeating(10);
        let rdef = rep.clone();
        let mut engine = TimerEngine::from_defs(vec![rep], t0());
        assert_eq!(
            engine.item_label(&rdef, t0()),
            "stretch - every 10 min, repeating (running, 10:00 until repeat)"
        );
        engine.toggle(&rdef.id, t0());
        assert_eq!(
            engine.item_label(&rdef, t0()),
            "stretch - every 10 min, repeating (stopped)"
        );
    }
}
