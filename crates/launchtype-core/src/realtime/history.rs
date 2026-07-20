//! Last-reading history for numeric realtime items (`realtime_history.json`):
//! compare a fresh value against the stored one, phrase the change, and
//! persist the new reading. The file shape (`{key: {value, timestamp}}`) and
//! bytes match the Python app so histories survive the rewrite.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

use crate::i18n::{format_args, tr, Arg};
use crate::storage::atomic_write_json;

use super::number::{format_number, python_float, python_round};

/// File name in the working directory (Python `HISTORY_FILE`).
pub const HISTORY_FILE: &str = "realtime_history.json";

/// Load the history document; a missing, corrupt or non-object file yields an
/// empty map (Python `_load_history`).
pub fn load_history(path: &Path) -> serde_json::Map<String, Value> {
    let parsed = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    match parsed {
        Some(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

/// Render an elapsed-seconds count as a speakable "... ago" phrase
/// (Python `_format_elapsed`).
pub fn format_elapsed(seconds: f64) -> String {
    let seconds = if seconds.is_finite() { seconds.trunc() as i64 } else { 0 }.max(0);
    if seconds < 60 {
        return tr("a few seconds ago");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        if minutes == 1 {
            return tr("1 minute ago");
        }
        return format_args(&tr("{count} minutes ago"), &[("count", Arg::Int(minutes))]);
    }
    let hours = minutes / 60;
    if hours < 24 {
        if hours == 1 {
            return tr("1 hour ago");
        }
        return format_args(&tr("{count} hours ago"), &[("count", Arg::Int(hours))]);
    }
    let days = hours / 24;
    if days == 1 {
        return tr("1 day ago");
    }
    format_args(&tr("{count} days ago"), &[("count", Arg::Int(days))])
}

/// The change-vs-previous phrase, or "" when there is nothing usable to
/// compare against (no previous entry, or one with missing/invalid fields).
pub fn delta_phrase(previous: Option<&Value>, current: f64, unit: &str, now_epoch: f64) -> String {
    let Some(previous) = previous.and_then(Value::as_object) else {
        return String::new();
    };
    let (Some(previous_value), Some(previous_time)) = (
        previous.get("value").and_then(python_float),
        previous.get("timestamp").and_then(python_float),
    ) else {
        return String::new();
    };

    let elapsed = format_elapsed(now_epoch - previous_time);
    let difference = current - previous_value;
    if python_round(difference, 2) == 0.0 {
        return format_args(&tr("unchanged since {elapsed}"), &[("elapsed", Arg::Str(&elapsed))]);
    }

    let amount = format_number(difference.abs(), 2);
    let percent = if previous_value != 0.0 {
        format_number(difference.abs() / previous_value.abs() * 100.0, 2)
    } else {
        format_number(0.0, 2)
    };

    let msgid = if difference > 0.0 {
        tr("up {amount} {unit} ({percent} percent) since {elapsed}")
    } else {
        tr("down {amount} {unit} ({percent} percent) since {elapsed}")
    };
    format_args(
        &msgid,
        &[
            ("amount", Arg::Str(&amount)),
            ("unit", Arg::Str(unit)),
            ("percent", Arg::Str(&percent)),
            ("elapsed", Arg::Str(&elapsed)),
        ],
    )
}

/// Serialised access to the history file (Python's `_HISTORY_LOCK` plus
/// `_compare_and_store`); safe to share between background fetches.
pub struct HistoryStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl HistoryStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        HistoryStore { path: path.into(), lock: Mutex::new(()) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Compare `current` against the last stored reading for `key`, persist
    /// the new reading, and return the speakable change phrase ("" when there
    /// is no previous reading to compare to).
    pub fn compare_and_store(
        &self,
        key: &str,
        current: f64,
        unit: &str,
        now_epoch: f64,
    ) -> io::Result<String> {
        let _guard = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut history = load_history(&self.path);
        let previous = history.get(key).cloned();
        history.insert(key.to_string(), new_entry(current, now_epoch));
        atomic_write_json(&self.path, &Value::Object(history), None)?;
        Ok(delta_phrase(previous.as_ref(), current, unit, now_epoch))
    }

    /// Append the change-vs-last-reading phrase to `sentence` when available
    /// (Python `_with_comparison`).
    pub fn with_comparison(
        &self,
        sentence: String,
        key: &str,
        current: f64,
        unit: &str,
        now_epoch: f64,
    ) -> io::Result<String> {
        let comparison = self.compare_and_store(key, current, unit, now_epoch)?;
        if comparison.is_empty() {
            Ok(sentence)
        } else {
            Ok(sentence + ", " + &comparison)
        }
    }
}

/// The `{"value": .., "timestamp": ..}` entry written for a reading, in
/// Python's key order.
fn new_entry(value: f64, timestamp: f64) -> Value {
    let mut entry = serde_json::Map::new();
    entry.insert("value".to_string(), number_value(value));
    entry.insert("timestamp".to_string(), number_value(timestamp));
    Value::Object(entry)
}

fn number_value(value: f64) -> Value {
    serde_json::Number::from_f64(value).map(Value::Number).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store(dir: &tempfile::TempDir) -> HistoryStore {
        HistoryStore::new(dir.path().join(HISTORY_FILE))
    }

    #[test]
    fn first_reading_has_no_phrase_and_writes_python_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(&dir);
        let phrase = history.compare_and_store("bitcoin", 100.5, "euros", 1753000000.25).unwrap();
        assert_eq!(phrase, "");
        let bytes = std::fs::read_to_string(history.path()).unwrap();
        // Exactly what Python's json.dumps produces for the same dict.
        assert_eq!(bytes, r#"{"bitcoin": {"value": 100.5, "timestamp": 1753000000.25}}"#);
    }

    #[test]
    fn rise_fall_and_unchanged_phrases() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(&dir);
        history.compare_and_store("bitcoin", 100.0, "euros", 880.0).unwrap();

        let up = history.compare_and_store("bitcoin", 105.5, "euros", 1000.0).unwrap();
        assert_eq!(up, "up 5.5 euros (5.5 percent) since 2 minutes ago");

        let down = history.compare_and_store("bitcoin", 103.5, "euros", 1030.0).unwrap();
        assert_eq!(down, "down 2 euros (1.9 percent) since a few seconds ago");

        // Differences that round to 0.00 count as unchanged.
        let same = history.compare_and_store("bitcoin", 103.504, "euros", 1090.0).unwrap();
        assert_eq!(same, "unchanged since 1 minute ago");
    }

    #[test]
    fn zero_previous_value_speaks_zero_percent() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(&dir);
        history.compare_and_store("ibex", 0.0, "points", 0.0).unwrap();
        let phrase = history.compare_and_store("ibex", 5.0, "points", 30.0).unwrap();
        assert_eq!(phrase, "up 5 points (0 percent) since a few seconds ago");
    }

    #[test]
    fn with_comparison_joins_with_comma() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(&dir);
        let first = history
            .with_comparison("base sentence".to_string(), "gold", 10.0, "us dollars", 100.0)
            .unwrap();
        assert_eq!(first, "base sentence");
        let second = history
            .with_comparison("base sentence".to_string(), "gold", 11.0, "us dollars", 160.0)
            .unwrap();
        assert_eq!(second, "base sentence, up 1 us dollars (10 percent) since 1 minute ago");
    }

    #[test]
    fn corrupt_file_and_malformed_entries_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(&dir);

        std::fs::write(history.path(), "not json").unwrap();
        assert_eq!(history.compare_and_store("a", 1.0, "euros", 0.0).unwrap(), "");

        // Non-dict previous entry.
        std::fs::write(history.path(), r#"{"a": 5}"#).unwrap();
        assert_eq!(history.compare_and_store("a", 1.0, "euros", 0.0).unwrap(), "");

        // Entry missing the timestamp.
        std::fs::write(history.path(), r#"{"a": {"value": 1.0}}"#).unwrap();
        assert_eq!(history.compare_and_store("a", 2.0, "euros", 0.0).unwrap(), "");

        // Entry with a non-numeric value.
        std::fs::write(history.path(), r#"{"a": {"value": "x", "timestamp": 1.0}}"#).unwrap();
        assert_eq!(history.compare_and_store("a", 2.0, "euros", 10.0).unwrap(), "");
    }

    #[test]
    fn preserves_other_keys_and_ascii_escaping() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(&dir);
        std::fs::write(history.path(), "{\"a\\u00f1o\": {\"value\": 1, \"timestamp\": 2}}").unwrap();
        history.compare_and_store("bitcoin", 3.0, "euros", 4.0).unwrap();
        let bytes = std::fs::read_to_string(history.path()).unwrap();
        assert_eq!(
            bytes,
            "{\"a\\u00f1o\": {\"value\": 1, \"timestamp\": 2}, \"bitcoin\": {\"value\": 3.0, \"timestamp\": 4.0}}"
        );
    }

    #[test]
    fn elapsed_phrasings_cover_every_bucket() {
        assert_eq!(format_elapsed(-5.0), "a few seconds ago");
        assert_eq!(format_elapsed(0.0), "a few seconds ago");
        assert_eq!(format_elapsed(59.9), "a few seconds ago");
        assert_eq!(format_elapsed(60.0), "1 minute ago");
        assert_eq!(format_elapsed(119.0), "1 minute ago");
        assert_eq!(format_elapsed(120.0), "2 minutes ago");
        assert_eq!(format_elapsed(3599.0), "59 minutes ago");
        assert_eq!(format_elapsed(3600.0), "1 hour ago");
        assert_eq!(format_elapsed(7200.0), "2 hours ago");
        assert_eq!(format_elapsed(86399.0), "23 hours ago");
        assert_eq!(format_elapsed(86400.0), "1 day ago");
        assert_eq!(format_elapsed(345600.0), "4 days ago");
    }

    #[test]
    fn delta_phrase_accepts_numeric_strings_like_python_float() {
        let previous = json!({"value": "100", "timestamp": "880"});
        let phrase = delta_phrase(Some(&previous), 105.5, "euros", 1000.0);
        assert_eq!(phrase, "up 5.5 euros (5.5 percent) since 2 minutes ago");
    }
}
