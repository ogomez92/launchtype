//! Timer engine — countdowns (a pure port of `services/timer_service.py`)
//! and stopwatches, which sound every so often until stopped.
//! Definitions persist to `timers.json`; live deadlines are in-memory only.
//! The background thread lives in `launchtype-services`; this engine just
//! answers "what fires now?" against an injected clock.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Local};
use serde::{Deserialize, Serialize};

use crate::i18n::{format_args, tr, Arg};

/// One entry of `timers.json`. Countdowns keep the Python dict's shape; a
/// stopwatch is an entry of its own `type`, with the fields only it has.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(try_from = "TimerRecord", into = "TimerRecord")]
pub struct TimerDef {
    pub id: String,
    pub title: String,
    pub description: String,
    pub kind: TimerKind,
    pub sound: Option<String>,
}

/// What a timer does once it is running.
#[derive(Debug, Clone, PartialEq)]
pub enum TimerKind {
    /// Counts `minutes` down and sounds once. A repeating countdown starts
    /// over each time it fires.
    Countdown { minutes: u64, repeating: bool },
    /// Has no end: counts up from when it is started and sounds every
    /// `every` `unit`s until it is stopped. Several can run side by side, say
    /// one sounding each minute of a talk and another marking every fifth.
    Stopwatch { every: u64, unit: TickUnit },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TickUnit {
    Second,
    Minute,
    Hour,
}

impl TickUnit {
    /// In the order the timer dialog lists them.
    pub const ALL: [TickUnit; 3] = [TickUnit::Second, TickUnit::Minute, TickUnit::Hour];

    fn seconds(self) -> i64 {
        match self {
            TickUnit::Second => 1,
            TickUnit::Minute => 60,
            TickUnit::Hour => 3600,
        }
    }
}

const COUNTDOWN_TYPE: &str = "timer";
const STOPWATCH_TYPE: &str = "stopwatch";

fn countdown_type() -> String {
    COUNTDOWN_TYPE.to_string()
}

/// The on-disk shape of a `TimerDef`: every kind's fields, each written only
/// by the kind that has it, so a countdown saves exactly as it always has.
#[derive(Serialize, Deserialize)]
struct TimerRecord {
    id: String,
    #[serde(rename = "type", default = "countdown_type")]
    kind: String,
    title: String,
    description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    minutes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repeating: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    every: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unit: Option<TickUnit>,
    #[serde(default)]
    sound: Option<String>,
}

impl TryFrom<TimerRecord> for TimerDef {
    type Error = String;

    fn try_from(record: TimerRecord) -> Result<Self, String> {
        let missing = |field: &str| format!("timer {} has no {field}", record.id);
        // Anything that is not a stopwatch is a countdown, as every entry was
        // before stopwatches existed, whatever its `type` said.
        let kind = if record.kind == STOPWATCH_TYPE {
            TimerKind::Stopwatch {
                every: record.every.ok_or_else(|| missing("every"))?,
                unit: record.unit.ok_or_else(|| missing("unit"))?,
            }
        } else {
            TimerKind::Countdown {
                minutes: record.minutes.ok_or_else(|| missing("minutes"))?,
                repeating: record.repeating.ok_or_else(|| missing("repeating"))?,
            }
        };
        Ok(TimerDef {
            id: record.id,
            title: record.title,
            description: record.description,
            kind,
            sound: record.sound,
        })
    }
}

impl From<TimerDef> for TimerRecord {
    fn from(def: TimerDef) -> Self {
        let mut record = TimerRecord {
            id: def.id,
            kind: countdown_type(),
            title: def.title,
            description: def.description,
            minutes: None,
            repeating: None,
            every: None,
            unit: None,
            sound: def.sound,
        };
        match def.kind {
            TimerKind::Countdown { minutes, repeating } => {
                record.minutes = Some(minutes);
                record.repeating = Some(repeating);
            }
            TimerKind::Stopwatch { every, unit } => {
                record.kind = STOPWATCH_TYPE.to_string();
                record.every = Some(every);
                record.unit = Some(unit);
            }
        }
        record
    }
}

impl TimerDef {
    pub fn new(title: String, description: String, kind: TimerKind, sound: Option<String>) -> Self {
        TimerDef { id: uuid::Uuid::new_v4().to_string(), title, description, kind, sound }
    }

    /// Time from starting (or firing) to the next fire.
    pub fn period(&self) -> Duration {
        match self.kind {
            TimerKind::Countdown { minutes, .. } => Duration::seconds(minutes as i64 * 60),
            TimerKind::Stopwatch { every, unit } => {
                Duration::seconds(every.max(1) as i64 * unit.seconds())
            }
        }
    }

    pub fn is_stopwatch(&self) -> bool {
        matches!(self.kind, TimerKind::Stopwatch { .. })
    }

    /// Whether firing schedules the next fire rather than stopping it.
    fn repeats(&self) -> bool {
        match self.kind {
            TimerKind::Countdown { repeating, .. } => repeating,
            TimerKind::Stopwatch { .. } => true,
        }
    }

    /// Repeating countdowns come up running (parity with Python). A stopwatch
    /// does not: one sounding away the moment the app starts would be noise.
    fn starts_by_itself(&self) -> bool {
        matches!(self.kind, TimerKind::Countdown { repeating: true, .. })
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
    /// When each running stopwatch was started, for the elapsed time its
    /// list label shows.
    started: HashMap<String, DateTime<Local>>,
}

impl TimerEngine {
    /// Repeating countdowns default to on whenever they are loaded.
    pub fn from_defs(timers: Vec<TimerDef>, now: DateTime<Local>) -> Self {
        let mut next_fire = HashMap::new();
        for t in &timers {
            if t.starts_by_itself() {
                next_fire.insert(t.id.clone(), Some(now + t.period()));
            }
        }
        TimerEngine { timers, next_fire, started: HashMap::new() }
    }

    /// Repeating countdowns start on by default.
    pub fn add(&mut self, def: TimerDef, now: DateTime<Local>) {
        if def.starts_by_itself() {
            self.next_fire.insert(def.id.clone(), Some(now + def.period()));
        }
        self.timers.push(def);
    }

    /// Replace a timer's definition in place, keeping its list position.
    /// A timer that was running restarts against the new period: leaving
    /// the old deadline would ignore an edited `minutes` until the next fire.
    /// A stopped timer stays stopped, and a running stopwatch keeps counting
    /// from when it was started. Returns false for an unknown id.
    pub fn update(&mut self, def: TimerDef, now: DateTime<Local>) -> bool {
        let Some(slot) = self.timers.iter_mut().find(|t| t.id == def.id) else {
            return false;
        };
        let deadline = self.next_fire.get(&def.id).and_then(|d| *d).map(|_| now + def.period());
        let id = def.id.clone();
        if deadline.is_some() && def.is_stopwatch() {
            self.started.entry(id.clone()).or_insert(now);
        } else {
            self.started.remove(&id);
        }
        *slot = def;
        self.next_fire.insert(id, deadline);
        true
    }

    /// Run/toggle a timer. Returns the new active state, or `None` for an
    /// unknown id. Non-repeating countdowns (re)start each run; repeating ones
    /// and stopwatches switch on and off.
    pub fn toggle(&mut self, timer_id: &str, now: DateTime<Local>) -> Option<bool> {
        let timer = self.timers.iter().find(|t| t.id == timer_id)?;
        if timer.repeats() && self.is_active(timer_id) {
            self.next_fire.insert(timer_id.to_string(), None);
            self.started.remove(timer_id);
            return Some(false);
        }
        if timer.is_stopwatch() {
            self.started.insert(timer_id.to_string(), now);
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

    /// Seconds since a running stopwatch was started, or `None` when it is
    /// stopped (or is not a stopwatch).
    pub fn elapsed_seconds(&self, timer_id: &str, now: DateTime<Local>) -> Option<i64> {
        let started = self.started.get(timer_id)?;
        Some((now - *started).num_seconds().max(0))
    }

    pub fn remove(&mut self, timer_id: &str) {
        self.timers.retain(|t| t.id != timer_id);
        self.next_fire.remove(timer_id);
        self.started.remove(timer_id);
    }

    /// Timers whose deadline has passed; repeating countdowns and stopwatches
    /// reschedule from `now`, one-shot ones deactivate.
    pub fn due(&mut self, now: DateTime<Local>) -> Vec<TimerDef> {
        let mut fired = Vec::new();
        for timer in &self.timers {
            let deadline = match self.next_fire.get(&timer.id) {
                Some(Some(d)) => *d,
                _ => continue,
            };
            if now >= deadline {
                fired.push(timer.clone());
                let next = if timer.repeats() { Some(now + timer.period()) } else { None };
                self.next_fire.insert(timer.id.clone(), next);
            }
        }
        fired
    }

    /// The localized "{title} - {descriptor} ({state})" list label for a timer.
    pub fn item_label(&self, timer: &TimerDef, now: DateTime<Local>) -> String {
        let (descriptor, state) = match timer.kind {
            TimerKind::Countdown { minutes, repeating } => {
                let state = match self.remaining_seconds(&timer.id, now) {
                    Some(remaining) => {
                        let verb = if repeating { tr("until repeat") } else { tr("left") };
                        running_state(remaining, &verb)
                    }
                    None => tr("stopped"),
                };
                let minutes = [("minutes", Arg::Int(minutes as i64))];
                let descriptor = if repeating {
                    format_args(&tr("every {minutes} min, repeating"), &minutes)
                } else {
                    format_args(&tr("{minutes} min"), &minutes)
                };
                (descriptor, state)
            }
            TimerKind::Stopwatch { every, unit } => {
                let state = match self.elapsed_seconds(&timer.id, now) {
                    Some(elapsed) => running_state(elapsed, &tr("elapsed")),
                    None => tr("stopped"),
                };
                let template = match unit {
                    TickUnit::Second => tr("stopwatch, sounds every {count} sec"),
                    TickUnit::Minute => tr("stopwatch, sounds every {count} min"),
                    TickUnit::Hour => tr("stopwatch, sounds every {count} h"),
                };
                (format_args(&template, &[("count", Arg::Int(every as i64))]), state)
            }
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

/// "running, 4:59 left" and the like.
fn running_state(seconds: i64, verb: &str) -> String {
    format_args(
        &tr("running, {time} {verb}"),
        &[("time", Arg::Str(&format_remaining(seconds))), ("verb", Arg::Str(verb))],
    )
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
        TimerDef::new(
            "tea".into(),
            "tea is ready".into(),
            TimerKind::Countdown { minutes, repeating: false },
            None,
        )
    }

    fn repeating(minutes: u64) -> TimerDef {
        TimerDef::new(
            "stretch".into(),
            "stand up".into(),
            TimerKind::Countdown { minutes, repeating: true },
            None,
        )
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
    fn update_rewrites_the_def_in_place() {
        let timer = one_shot(5);
        let id = timer.id.clone();
        let mut engine = TimerEngine::from_defs(vec![timer], t0());

        let mut edited = engine.timers[0].clone();
        edited.title = "coffee".into();
        edited.kind = TimerKind::Countdown { minutes: 9, repeating: false };
        edited.sound = Some("timers/marimba.wav".into());
        assert!(engine.update(edited, t0()));

        assert_eq!(engine.timers.len(), 1, "the edit replaces, it does not append");
        assert_eq!(engine.timers[0].id, id, "the id survives an edit");
        assert_eq!(engine.timers[0].title, "coffee");
        assert_eq!(engine.timers[0].sound.as_deref(), Some("timers/marimba.wav"));
    }

    #[test]
    fn update_reschedules_only_a_running_countdown() {
        let timer = one_shot(5);
        let id = timer.id.clone();
        let mut engine = TimerEngine::from_defs(vec![timer], t0());
        let mut edited = engine.timers[0].clone();
        edited.kind = TimerKind::Countdown { minutes: 9, repeating: false };

        // Stopped stays stopped.
        assert!(engine.update(edited.clone(), t0()));
        assert!(!engine.is_active(&id));

        // Running picks up the new period from the moment of the edit.
        engine.toggle(&id, t0());
        let later = t0() + Duration::seconds(60);
        edited.kind = TimerKind::Countdown { minutes: 2, repeating: false };
        assert!(engine.update(edited, later));
        assert_eq!(engine.remaining_seconds(&id, later), Some(120));

        assert!(!engine.update(one_shot(1), t0()), "unknown id");
    }

    #[test]
    fn json_round_trip_matches_python_shape() {
        let json = r#"[{"id": "abc", "type": "timer", "title": "tea", "description": "ready", "minutes": 3, "repeating": false, "sound": "ding.wav"}]"#;
        let defs: Vec<TimerDef> = serde_json::from_str(json).unwrap();
        assert_eq!(defs[0].kind, TimerKind::Countdown { minutes: 3, repeating: false });
        let out = crate::storage::to_python_json(&defs, None).unwrap();
        assert_eq!(out, json);
    }

    fn stopwatch(every: u64, unit: TickUnit) -> TimerDef {
        TimerDef::new(
            "talk".into(),
            String::new(),
            TimerKind::Stopwatch { every, unit },
            Some("stopwatch/clock-tick.wav".into()),
        )
    }

    #[test]
    fn stopwatch_starts_stopped_and_sounds_until_stopped() {
        let timer = stopwatch(5, TickUnit::Minute);
        let id = timer.id.clone();
        let mut engine = TimerEngine::from_defs(vec![timer], t0());
        assert!(!engine.is_active(&id), "a stopwatch never starts by itself");
        assert!(engine.due(t0() + Duration::hours(1)).is_empty());

        assert_eq!(engine.toggle(&id, t0()), Some(true));
        for fire in 1..=3 {
            let at = t0() + Duration::minutes(5 * fire);
            assert!(engine.due(at - Duration::seconds(1)).is_empty());
            assert_eq!(engine.due(at).len(), 1, "fire {fire}");
        }
        assert_eq!(engine.elapsed_seconds(&id, t0() + Duration::minutes(15)), Some(900));

        assert_eq!(engine.toggle(&id, t0() + Duration::minutes(16)), Some(false));
        assert_eq!(engine.elapsed_seconds(&id, t0() + Duration::minutes(16)), None);
        assert!(engine.due(t0() + Duration::hours(2)).is_empty());
    }

    #[test]
    fn stopwatches_run_side_by_side() {
        let every_minute = stopwatch(1, TickUnit::Minute);
        let every_five = stopwatch(5, TickUnit::Minute);
        let (a, b) = (every_minute.id.clone(), every_five.id.clone());
        let mut engine = TimerEngine::from_defs(vec![every_minute, every_five], t0());
        engine.toggle(&a, t0());
        engine.toggle(&b, t0());
        let mut fired = 0;
        let mut fired_five = 0;
        for second in 1..=300 {
            for def in engine.due(t0() + Duration::seconds(second)) {
                fired += 1;
                if def.id == b {
                    fired_five += 1;
                }
            }
        }
        assert_eq!((fired, fired_five), (6, 1));
    }

    #[test]
    fn stopwatch_every_second() {
        let timer = stopwatch(1, TickUnit::Second);
        let id = timer.id.clone();
        let mut engine = TimerEngine::from_defs(vec![timer], t0());
        engine.toggle(&id, t0());
        let fires: usize =
            (1..=10).map(|s| engine.due(t0() + Duration::seconds(s)).len()).sum();
        assert_eq!(fires, 10);
    }

    #[test]
    fn stopwatch_is_its_own_json_entry() {
        let json = r#"[{"id": "abc", "type": "timer", "title": "tea", "description": "ready", "minutes": 3, "repeating": false, "sound": null}, {"id": "def", "type": "stopwatch", "title": "talk", "description": "", "every": 5, "unit": "minute", "sound": "stopwatch/glass-ping.wav"}]"#;
        let defs: Vec<TimerDef> = serde_json::from_str(json).unwrap();
        assert_eq!(defs[1].kind, TimerKind::Stopwatch { every: 5, unit: TickUnit::Minute });
        let out = crate::storage::to_python_json(&defs, None).unwrap();
        assert_eq!(out, json);
    }

    #[test]
    fn entries_without_a_type_are_countdowns() {
        let json = r#"[{"id": "abc", "title": "tea", "description": "", "minutes": 3, "repeating": true}]"#;
        let defs: Vec<TimerDef> = serde_json::from_str(json).unwrap();
        assert_eq!(defs[0].kind, TimerKind::Countdown { minutes: 3, repeating: true });
        assert_eq!(defs[0].sound, None);
    }

    #[test]
    fn stopwatch_label_english() {
        let timer = stopwatch(5, TickUnit::Minute);
        let def = timer.clone();
        let mut engine = TimerEngine::from_defs(vec![timer], t0());
        assert_eq!(engine.item_label(&def, t0()), "talk - stopwatch, sounds every 5 min (stopped)");
        engine.toggle(&def.id, t0());
        assert_eq!(
            engine.item_label(&def, t0() + Duration::seconds(75)),
            "talk - stopwatch, sounds every 5 min (running, 1:15 elapsed)"
        );
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
