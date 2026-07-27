//! Claude Code and Codex subscription usage responses turned into spoken
//! summaries. The current instant is injected so relative reset moments ("in
//! 5 days") are testable; production callers pass `&chrono::Local::now()`.

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde_json::Value;

use crate::i18n::{format_args, tr, Arg};

use super::number::{format_number, python_float};
use super::{parse_json_body, RealtimeError};

/// The endpoint behind Claude Code's /usage command.
pub const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// Value of the `anthropic-beta` header the usage query needs.
pub const CLAUDE_OAUTH_BETA: &str = "oauth-2025-04-20";

pub const CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
/// OpenAI's public OAuth client id for Codex, hardcoded in the Codex CLI.
pub const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

/// Turn an ISO reset timestamp into a localized *relative* phrase measured
/// from `now` — "in 5 days", "in 3 hours 20 minutes" — or `None` if it is
/// unparseable. Naive timestamps (no offset) are read as local wall time,
/// interpreted in `now`'s timezone (Python's `astimezone()` treats them as
/// already-local).
pub fn format_reset_moment<Tz>(value: Option<&Value>, now: &DateTime<Tz>) -> Option<String>
where
    Tz: TimeZone,
{
    let text = value?.as_str()?;
    let target = if let Ok(aware) = DateTime::parse_from_rfc3339(text) {
        aware.with_timezone(&Utc)
    } else {
        now.timezone()
            .from_local_datetime(&parse_naive(text)?)
            .single()?
            .with_timezone(&Utc)
    };
    Some(humanize_until(target, now))
}

fn parse_naive(text: &str) -> Option<NaiveDateTime> {
    for pattern in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, pattern) {
            return Some(naive);
        }
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?.and_hms_opt(0, 0, 0)
}

/// Turn a unix-epoch reset timestamp into a localized relative phrase (see
/// [`format_reset_moment`]), or `None` if invalid — negative or absurd values
/// fail like `datetime.fromtimestamp` on Windows.
pub fn format_epoch_moment<Tz>(value: Option<&Value>, now: &DateTime<Tz>) -> Option<String>
where
    Tz: TimeZone,
{
    let seconds = python_float(value?)?;
    // Windows fromtimestamp rejects pre-epoch values; year 9999 is the cap.
    if !seconds.is_finite() || !(0.0..=253_402_300_799.0).contains(&seconds) {
        return None;
    }
    let whole = seconds.trunc() as i64;
    let nanos = (((seconds - whole as f64) * 1e9).round() as u32).min(999_999_999);
    let target = DateTime::<Utc>::from_timestamp(whole, nanos)?;
    Some(humanize_until(target, now))
}

/// Localized "in {duration}" for the gap between `now` and `target`, using the
/// two most significant non-zero units (days+hours, hours+minutes, or
/// minutes). Sub-minute gaps — and resets already due — read "in less than a
/// minute".
fn humanize_until<Tz>(target: DateTime<Utc>, now: &DateTime<Tz>) -> String
where
    Tz: TimeZone,
{
    let seconds = (target - now.with_timezone(&Utc)).num_seconds();
    if seconds < 60 {
        return tr("in less than a minute");
    }
    let total_minutes = seconds / 60;
    let days = total_minutes / (24 * 60);
    let hours = (total_minutes % (24 * 60)) / 60;
    let minutes = total_minutes % 60;

    let duration = if days >= 1 {
        join_units(unit(days, Unit::Day), (hours > 0).then(|| unit(hours, Unit::Hour)))
    } else if hours >= 1 {
        join_units(unit(hours, Unit::Hour), (minutes > 0).then(|| unit(minutes, Unit::Minute)))
    } else {
        unit(minutes, Unit::Minute)
    };
    format_args(&tr("in {duration}"), &[("duration", Arg::Str(&duration))])
}

fn join_units(major: String, minor: Option<String>) -> String {
    match minor {
        Some(minor) => format!("{major} {minor}"),
        None => major,
    }
}

#[derive(Clone, Copy)]
enum Unit {
    Day,
    Hour,
    Minute,
}

/// One pluralized, localized count phrase like "5 days" or "1 hour".
fn unit(n: i64, kind: Unit) -> String {
    let template = match (kind, n == 1) {
        (Unit::Day, true) => tr("{n} day"),
        (Unit::Day, false) => tr("{n} days"),
        (Unit::Hour, true) => tr("{n} hour"),
        (Unit::Hour, false) => tr("{n} hours"),
        (Unit::Minute, true) => tr("{n} minute"),
        (Unit::Minute, false) => tr("{n} minutes"),
    };
    format_args(&template, &[("n", Arg::Int(n))])
}

fn utilization(section: Option<&Value>) -> Option<&Value> {
    section?.as_object()?.get("utilization").filter(|value| !value.is_null())
}

/// Python `_fetch_claude_usage`, minus the credential read and HTTP call:
/// turn the api.anthropic.com/api/oauth/usage response into the spoken
/// summary of the 5-hour session, 7-day week and opus-week windows.
pub fn claude_usage_sentence<Tz>(body: &str, now: &DateTime<Tz>) -> Result<String, RealtimeError>
where
    Tz: TimeZone,
{
    let body = parse_json_body(body)?;
    // Percentages lead the sentence (the numbers watched at a glance); the
    // reset moments trail behind them, so the whole line reads "all models X%,
    // weekly Y%, fable Z%, session resets in …, weekly resets in …".
    let mut percents: Vec<String> = Vec::new();
    let mut resets: Vec<String> = Vec::new();

    let session = body.get("five_hour");
    if let Some(value) = utilization(session) {
        let percent = format_number(python_float(value).ok_or(RealtimeError::NotUnderstood)?, 0);
        percents.push(format_args(&tr("all models {percent}%"), &[("percent", Arg::Str(&percent))]));
        if let Some(reset) = format_reset_moment(session.and_then(|s| s.get("resets_at")), now) {
            resets.push(format_args(
                &tr("session resets {reset}"),
                &[("reset", Arg::Str(&reset))],
            ));
        }
    }

    let week = body.get("seven_day");
    if let Some(value) = utilization(week) {
        let percent = format_number(python_float(value).ok_or(RealtimeError::NotUnderstood)?, 0);
        percents.push(format_args(&tr("weekly {percent}%"), &[("percent", Arg::Str(&percent))]));
        if let Some(reset) = format_reset_moment(week.and_then(|w| w.get("resets_at")), now) {
            resets.push(format_args(
                &tr("weekly resets {reset}"),
                &[("reset", Arg::Str(&reset))],
            ));
        }
    }

    // Per-model weekly caps, each as "{model} {percent}%".
    let mut scoped: Vec<String> = Vec::new();
    if let Some(value) = utilization(body.get("seven_day_opus")) {
        let percent = format_number(python_float(value).ok_or(RealtimeError::NotUnderstood)?, 0);
        scoped.push("opus".to_string());
        percents.push(format_args(
            &tr("{model} {percent}%"),
            &[("model", Arg::Str("opus")), ("percent", Arg::Str(&percent))],
        ));
    }

    // Newer responses report the per-model weekly cap (Fable, Opus, …) as a
    // `weekly_scoped` entry in `limits` instead of its own top-level section,
    // so the dedicated sections above can be null while this one is live.
    for limit in body.get("limits").and_then(Value::as_array).map_or(&[][..], Vec::as_slice) {
        if limit.get("kind").and_then(Value::as_str) != Some("weekly_scoped") {
            continue;
        }
        let Some(model) = limit
            .pointer("/scope/model/display_name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(str::to_lowercase)
        else {
            continue;
        };
        let Some(percent) = limit.get("percent").and_then(python_float) else {
            continue;
        };
        if scoped.contains(&model) {
            continue;
        }
        percents.push(format_args(
            &tr("{model} {percent}%"),
            &[
                ("model", Arg::Str(&model)),
                ("percent", Arg::Str(&format_number(percent, 0))),
            ],
        ));
        scoped.push(model);
    }

    if percents.is_empty() {
        return Err(RealtimeError::NotUnderstood);
    }
    let mut parts = percents;
    parts.extend(resets);
    Ok(format_args(&tr("Claude usage: {parts}"), &[("parts", Arg::Str(&parts.join(", ")))]))
}

/// Python `_codex_window_part`: the spoken phrase for one rate-limit window,
/// or `None`. The window's duration decides its label: up to a day it is the
/// session (reset as a time of day), around a week the week, longer the month
/// (reset with the date).
pub fn codex_window_part<Tz>(window: &Value, now: &DateTime<Tz>) -> Option<String>
where
    Tz: TimeZone,
{
    let window = window.as_object()?;
    let used = window.get("used_percent").filter(|value| !value.is_null())?;
    let percent = format_number(python_float(used)?, 0);

    let hours = window
        .get("limit_window_seconds")
        .map(|value| if python_truthy(value) { python_float(value).unwrap_or(0.0) } else { 0.0 })
        .unwrap_or(0.0)
        / 3600.0;

    if hours <= 24.0 {
        let reset = format_epoch_moment(window.get("reset_at"), now);
        return Some(match reset {
            Some(reset) => format_args(
                &tr("session at {percent} percent, resets {reset}"),
                &[("percent", Arg::Str(&percent)), ("reset", Arg::Str(&reset))],
            ),
            None => format_args(
                &tr("session at {percent} percent"),
                &[("percent", Arg::Str(&percent))],
            ),
        });
    }

    let reset = format_epoch_moment(window.get("reset_at"), now);
    if hours <= 10.0 * 24.0 {
        return Some(match reset {
            Some(reset) => format_args(
                &tr("week at {percent} percent, resets {reset}"),
                &[("percent", Arg::Str(&percent)), ("reset", Arg::Str(&reset))],
            ),
            None => format_args(
                &tr("week at {percent} percent"),
                &[("percent", Arg::Str(&percent))],
            ),
        });
    }

    Some(match reset {
        Some(reset) => format_args(
            &tr("month at {percent} percent, resets {reset}"),
            &[("percent", Arg::Str(&percent)), ("reset", Arg::Str(&reset))],
        ),
        None => format_args(
            &tr("month at {percent} percent"),
            &[("percent", Arg::Str(&percent))],
        ),
    })
}

fn python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python `_fetch_openai_usage`, minus the token handling and HTTP call:
/// turn the chatgpt.com/backend-api/wham/usage response into the spoken
/// summary, windows labelled from their duration.
pub fn openai_usage_sentence<Tz>(body: &str, now: &DateTime<Tz>) -> Result<String, RealtimeError>
where
    Tz: TimeZone,
{
    let body = parse_json_body(body)?;
    let mut parts: Vec<String> = Vec::new();

    let rate_limit = body.get("rate_limit").and_then(Value::as_object);
    for window_key in ["primary_window", "secondary_window"] {
        if let Some(window) = rate_limit.and_then(|limit| limit.get(window_key)) {
            if let Some(part) = codex_window_part(window, now) {
                parts.push(part);
            }
        }
    }

    if parts.is_empty() {
        return Err(RealtimeError::NotUnderstood);
    }

    if let Some(plan) = body.get("plan_type").and_then(Value::as_str).filter(|p| !p.is_empty()) {
        parts.insert(0, format_args(&tr("{plan} plan"), &[("plan", Arg::Str(plan))]));
    }

    Ok(format_args(&tr("OpenAI usage: {parts}"), &[("parts", Arg::Str(&parts.join(", ")))]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use serde_json::json;

    fn plus2() -> FixedOffset {
        FixedOffset::east_opt(2 * 3600).unwrap()
    }

    /// A fixed `now` in the +02:00 zone; production passes `Local::now()`.
    fn now_at(month: u32, day: u32, hour: u32, minute: u32) -> DateTime<FixedOffset> {
        plus2().with_ymd_and_hms(2026, month, day, hour, minute, 0).unwrap()
    }

    #[test]
    fn claude_sentence_exact() {
        // now = 2026-07-20 10:00 UTC: session reset 8h30m out, weekly 4d out.
        let now = now_at(7, 20, 12, 0);
        let body = r#"{
            "five_hour": {"utilization": 42, "resets_at": "2026-07-20T18:30:00+00:00"},
            "seven_day": {"utilization": 81.4, "resets_at": "2026-07-24T10:00:00+00:00"},
            "seven_day_opus": {"utilization": 12}
        }"#;
        assert_eq!(
            claude_usage_sentence(body, &now).unwrap(),
            "Claude usage: all models 42%, weekly 81%, opus 12%, \
             session resets in 8 hours 30 minutes, weekly resets in 4 days"
        );
    }

    #[test]
    fn claude_scoped_weekly_limit_is_reported() {
        // The live shape: dedicated model sections null, the model cap in `limits`.
        // now = 2026-07-20 10:00 UTC: session reset 1d8h out, weekly 3d11h out.
        let now = now_at(7, 20, 12, 0);
        let body = r#"{
            "five_hour": {"utilization": 14, "resets_at": "2026-07-21T18:00:00+00:00"},
            "seven_day": {"utilization": 40, "resets_at": "2026-07-23T21:00:00+00:00"},
            "seven_day_opus": null,
            "limits": [
                {"kind": "session", "percent": 14},
                {"kind": "weekly_all", "percent": 40},
                {"kind": "weekly_scoped", "percent": 47,
                 "scope": {"model": {"id": null, "display_name": "Fable"}, "surface": null}}
            ]
        }"#;
        assert_eq!(
            claude_usage_sentence(body, &now).unwrap(),
            "Claude usage: all models 14%, weekly 40%, fable 47%, \
             session resets in 1 day 8 hours, weekly resets in 3 days 11 hours"
        );
    }

    #[test]
    fn claude_scoped_weekly_limit_never_duplicates_a_section() {
        // `seven_day_opus` and a scoped opus limit describe the same window.
        let body = r#"{
            "seven_day_opus": {"utilization": 12},
            "limits": [
                {"kind": "weekly_scoped", "percent": 12,
                 "scope": {"model": {"display_name": "Opus"}}},
                {"kind": "weekly_scoped", "percent": 47,
                 "scope": {"model": {"display_name": "Fable"}}}
            ]
        }"#;
        assert_eq!(
            claude_usage_sentence(body, &now_at(7, 20, 12, 0)).unwrap(),
            "Claude usage: opus 12%, fable 47%"
        );
    }

    #[test]
    fn claude_scoped_limits_without_a_usable_model_are_skipped() {
        for limits in [
            r#"[{"kind": "weekly_scoped", "percent": 47}]"#,
            r#"[{"kind": "weekly_scoped", "percent": 47, "scope": null}]"#,
            r#"[{"kind": "weekly_scoped", "percent": 47, "scope": {"model": {"display_name": ""}}}]"#,
            r#"[{"kind": "weekly_scoped", "scope": {"model": {"display_name": "Fable"}}}]"#,
            r#"[{"kind": "weekly_all", "percent": 47,
                 "scope": {"model": {"display_name": "Fable"}}}]"#,
            r#"{"not": "an array"}"#,
        ] {
            let body = format!(r#"{{"five_hour": {{"utilization": 3}}, "limits": {limits}}}"#);
            assert_eq!(
                claude_usage_sentence(&body, &now_at(7, 20, 12, 0)).unwrap(),
                "Claude usage: all models 3%",
                "for {limits}"
            );
        }
    }

    #[test]
    fn claude_partial_sections_and_missing_resets() {
        let body = r#"{"five_hour": {"utilization": 0}}"#;
        assert_eq!(
            claude_usage_sentence(body, &now_at(7, 20, 12, 0)).unwrap(),
            "Claude usage: all models 0%"
        );
        let body = r#"{"seven_day": {"utilization": 99.6, "resets_at": null}}"#;
        assert_eq!(
            claude_usage_sentence(body, &now_at(7, 20, 12, 0)).unwrap(),
            "Claude usage: weekly 100%"
        );
    }

    #[test]
    fn claude_empty_or_null_sections_are_not_understood() {
        for body in [
            "{}",
            r#"{"five_hour": null, "seven_day": {}}"#,
            r#"{"five_hour": {"utilization": null}}"#,
            "not json",
        ] {
            let error = claude_usage_sentence(body, &now_at(7, 20, 12, 0)).unwrap_err();
            assert_eq!(error, RealtimeError::NotUnderstood, "for {body:?}");
        }
    }

    #[test]
    fn reset_moment_formats() {
        // now = 2026-07-20 10:00 UTC (12:00 in the +02:00 zone).
        let now = now_at(7, 20, 12, 0);
        let aware = json!("2026-07-20T18:30:00+00:00");
        assert_eq!(
            format_reset_moment(Some(&aware), &now),
            Some("in 8 hours 30 minutes".to_string())
        );
        // A whole number of days omits the trailing (zero) hours.
        let days = json!("2026-07-25T10:00:00Z");
        assert_eq!(format_reset_moment(Some(&days), &now), Some("in 5 days".to_string()));
        let zulu = json!("2026-07-20T23:30:00Z");
        assert_eq!(
            format_reset_moment(Some(&zulu), &now),
            Some("in 13 hours 30 minutes".to_string())
        );
        // Naive timestamps are read as local wall time in `now`'s zone (+02:00),
        // so 18:30 local is 16:30 UTC — 6h30m out.
        let naive = json!("2026-07-20T18:30:00");
        assert_eq!(
            format_reset_moment(Some(&naive), &now),
            Some("in 6 hours 30 minutes".to_string())
        );
        // A reset barely ahead (or already due) collapses to a single phrase.
        let imminent = json!("2026-07-20T10:00:30Z");
        assert_eq!(
            format_reset_moment(Some(&imminent), &now),
            Some("in less than a minute".to_string())
        );
        assert_eq!(format_reset_moment(None, &now), None);
        assert_eq!(format_reset_moment(Some(&json!(null)), &now), None);
        assert_eq!(format_reset_moment(Some(&json!(12345)), &now), None);
        assert_eq!(format_reset_moment(Some(&json!("garbage")), &now), None);
    }

    #[test]
    fn epoch_moment_formats() {
        // now = 2025-12-31T00:00:00Z; 1767225600 == 2026-01-01T00:00:00Z (1 day out).
        let now = Utc.with_ymd_and_hms(2025, 12, 31, 0, 0, 0).unwrap();
        let epoch = json!(1767225600);
        assert_eq!(format_epoch_moment(Some(&epoch), &now), Some("in 1 day".to_string()));
        // Numeric strings coerce like Python float().
        let text = json!("1767225600");
        assert_eq!(format_epoch_moment(Some(&text), &now), Some("in 1 day".to_string()));
        assert_eq!(format_epoch_moment(Some(&json!(-5)), &now), None);
        assert_eq!(format_epoch_moment(Some(&json!(1e18)), &now), None);
        assert_eq!(format_epoch_moment(Some(&json!(null)), &now), None);
        assert_eq!(format_epoch_moment(None, &now), None);
    }

    #[test]
    fn openai_sentence_exact() {
        // now = 2026-01-01T00:00:00Z (epoch 1767225600); primary resets 4h30m
        // out (1767241800), secondary 6 days out (1767744000).
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let body = r#"{
            "plan_type": "plus",
            "rate_limit": {
                "primary_window": {
                    "used_percent": 23.4,
                    "limit_window_seconds": 18000,
                    "reset_at": 1767241800
                },
                "secondary_window": {
                    "used_percent": 61.2,
                    "limit_window_seconds": 604800,
                    "reset_at": 1767744000
                }
            }
        }"#;
        assert_eq!(
            openai_usage_sentence(body, &now).unwrap(),
            "OpenAI usage: plus plan, session at 23 percent, resets in 4 hours 30 minutes, \
             week at 61 percent, resets in 6 days"
        );
    }

    #[test]
    fn codex_windows_label_by_duration() {
        let now = now_at(7, 20, 12, 0);
        // A 30-day window becomes the month; without reset_at, no reset clause.
        let month = json!({"used_percent": 5, "limit_window_seconds": 2592000});
        assert_eq!(codex_window_part(&month, &now), Some("month at 5 percent".to_string()));
        // Exactly 24 hours still counts as the session.
        let day = json!({"used_percent": 50, "limit_window_seconds": 86400});
        assert_eq!(codex_window_part(&day, &now), Some("session at 50 percent".to_string()));
        // Missing/null/zero duration defaults to the session label.
        let no_duration = json!({"used_percent": 7, "limit_window_seconds": null});
        assert_eq!(codex_window_part(&no_duration, &now), Some("session at 7 percent".to_string()));
        // An unparseable duration falls back to 0 hours, like Python's except.
        let bad_duration = json!({"used_percent": 7, "limit_window_seconds": "abc"});
        assert_eq!(codex_window_part(&bad_duration, &now), Some("session at 7 percent".to_string()));
        // Windows without used_percent are skipped, not errors.
        assert_eq!(codex_window_part(&json!({"limit_window_seconds": 18000}), &now), None);
        assert_eq!(codex_window_part(&json!({"used_percent": null}), &now), None);
        assert_eq!(codex_window_part(&json!({"used_percent": "n/a"}), &now), None);
        assert_eq!(codex_window_part(&json!("not a window"), &now), None);
        // A negative epoch drops the reset clause (Windows fromtimestamp).
        let bad_reset = json!({"used_percent": 9, "limit_window_seconds": 18000, "reset_at": -5});
        assert_eq!(codex_window_part(&bad_reset, &now), Some("session at 9 percent".to_string()));
    }

    #[test]
    fn openai_without_usable_windows_is_not_understood() {
        for body in [
            "{}",
            r#"{"rate_limit": {}}"#,
            r#"{"rate_limit": null, "plan_type": "plus"}"#,
            r#"{"rate_limit": {"primary_window": {"used_percent": null}}}"#,
        ] {
            let error = openai_usage_sentence(body, &now_at(7, 20, 12, 0)).unwrap_err();
            assert_eq!(error, RealtimeError::NotUnderstood, "for {body:?}");
        }
    }

    #[test]
    fn openai_plan_only_never_stands_alone() {
        // The plan is inserted only when at least one window part exists.
        let body = r#"{"plan_type": "pro", "rate_limit": {"primary_window": {"used_percent": 1}}}"#;
        assert_eq!(
            openai_usage_sentence(body, &now_at(7, 20, 12, 0)).unwrap(),
            "OpenAI usage: pro plan, session at 1 percent"
        );
    }
}
