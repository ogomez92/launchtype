//! Claude Code and Codex subscription usage responses turned into spoken
//! summaries. Timezone is injected so reset moments are testable; production
//! callers pass `&chrono::Local` to match Python's `.astimezone()`.

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

/// Python `_format_reset_moment`: format an ISO reset timestamp in local
/// time, or `None` if unparseable. Naive timestamps keep their wall time
/// (Python's `astimezone()` treats them as already-local).
pub fn format_reset_moment<Tz>(value: Option<&Value>, include_date: bool, tz: &Tz) -> Option<String>
where
    Tz: TimeZone,
    Tz::Offset: std::fmt::Display,
{
    let text = value?.as_str()?;
    let pattern = if include_date { "%d/%m %H:%M" } else { "%H:%M" };
    if let Ok(aware) = DateTime::parse_from_rfc3339(text) {
        return Some(aware.with_timezone(tz).format(pattern).to_string());
    }
    for naive_pattern in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, naive_pattern) {
            return Some(naive.format(pattern).to_string());
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return Some(date.and_hms_opt(0, 0, 0)?.format(pattern).to_string());
    }
    None
}

/// Python `_format_epoch_moment`: format a unix-epoch reset timestamp in
/// local time, or `None` if invalid (negative or absurd values fail, like
/// `datetime.fromtimestamp` on Windows).
pub fn format_epoch_moment<Tz>(value: Option<&Value>, include_date: bool, tz: &Tz) -> Option<String>
where
    Tz: TimeZone,
    Tz::Offset: std::fmt::Display,
{
    let seconds = python_float(value?)?;
    // Windows fromtimestamp rejects pre-epoch values; year 9999 is the cap.
    if !seconds.is_finite() || !(0.0..=253_402_300_799.0).contains(&seconds) {
        return None;
    }
    let whole = seconds.trunc() as i64;
    let nanos = (((seconds - whole as f64) * 1e9).round() as u32).min(999_999_999);
    let moment = DateTime::<Utc>::from_timestamp(whole, nanos)?.with_timezone(tz);
    let pattern = if include_date { "%d/%m %H:%M" } else { "%H:%M" };
    Some(moment.format(pattern).to_string())
}

fn utilization(section: Option<&Value>) -> Option<&Value> {
    section?.as_object()?.get("utilization").filter(|value| !value.is_null())
}

/// Python `_fetch_claude_usage`, minus the credential read and HTTP call:
/// turn the api.anthropic.com/api/oauth/usage response into the spoken
/// summary of the 5-hour session, 7-day week and opus-week windows.
pub fn claude_usage_sentence<Tz>(body: &str, tz: &Tz) -> Result<String, RealtimeError>
where
    Tz: TimeZone,
    Tz::Offset: std::fmt::Display,
{
    let body = parse_json_body(body)?;
    let mut parts: Vec<String> = Vec::new();

    let session = body.get("five_hour");
    if let Some(value) = utilization(session) {
        let percent = format_number(python_float(value).ok_or(RealtimeError::NotUnderstood)?, 0);
        let reset = format_reset_moment(
            session.and_then(|s| s.get("resets_at")),
            false,
            tz,
        );
        parts.push(match reset {
            Some(reset) => format_args(
                &tr("session at {percent} percent, resets at {reset}"),
                &[("percent", Arg::Str(&percent)), ("reset", Arg::Str(&reset))],
            ),
            None => format_args(
                &tr("session at {percent} percent"),
                &[("percent", Arg::Str(&percent))],
            ),
        });
    }

    let week = body.get("seven_day");
    if let Some(value) = utilization(week) {
        let percent = format_number(python_float(value).ok_or(RealtimeError::NotUnderstood)?, 0);
        let reset = format_reset_moment(week.and_then(|w| w.get("resets_at")), true, tz);
        parts.push(match reset {
            Some(reset) => format_args(
                &tr("week at {percent} percent, resets on {reset}"),
                &[("percent", Arg::Str(&percent)), ("reset", Arg::Str(&reset))],
            ),
            None => format_args(
                &tr("week at {percent} percent"),
                &[("percent", Arg::Str(&percent))],
            ),
        });
    }

    let mut scoped: Vec<String> = Vec::new();
    if let Some(value) = utilization(body.get("seven_day_opus")) {
        let percent = format_number(python_float(value).ok_or(RealtimeError::NotUnderstood)?, 0);
        scoped.push("opus".to_string());
        parts.push(format_args(
            &tr("opus week at {percent} percent"),
            &[("percent", Arg::Str(&percent))],
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
        parts.push(format_args(
            &tr("{model} week at {percent} percent"),
            &[
                ("model", Arg::Str(&model)),
                ("percent", Arg::Str(&format_number(percent, 0))),
            ],
        ));
        scoped.push(model);
    }

    if parts.is_empty() {
        return Err(RealtimeError::NotUnderstood);
    }
    Ok(format_args(&tr("Claude usage: {parts}"), &[("parts", Arg::Str(&parts.join(", ")))]))
}

/// Python `_codex_window_part`: the spoken phrase for one rate-limit window,
/// or `None`. The window's duration decides its label: up to a day it is the
/// session (reset as a time of day), around a week the week, longer the month
/// (reset with the date).
pub fn codex_window_part<Tz>(window: &Value, tz: &Tz) -> Option<String>
where
    Tz: TimeZone,
    Tz::Offset: std::fmt::Display,
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
        let reset = format_epoch_moment(window.get("reset_at"), false, tz);
        return Some(match reset {
            Some(reset) => format_args(
                &tr("session at {percent} percent, resets at {reset}"),
                &[("percent", Arg::Str(&percent)), ("reset", Arg::Str(&reset))],
            ),
            None => format_args(
                &tr("session at {percent} percent"),
                &[("percent", Arg::Str(&percent))],
            ),
        });
    }

    let reset = format_epoch_moment(window.get("reset_at"), true, tz);
    if hours <= 10.0 * 24.0 {
        return Some(match reset {
            Some(reset) => format_args(
                &tr("week at {percent} percent, resets on {reset}"),
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
            &tr("month at {percent} percent, resets on {reset}"),
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
pub fn openai_usage_sentence<Tz>(body: &str, tz: &Tz) -> Result<String, RealtimeError>
where
    Tz: TimeZone,
    Tz::Offset: std::fmt::Display,
{
    let body = parse_json_body(body)?;
    let mut parts: Vec<String> = Vec::new();

    let rate_limit = body.get("rate_limit").and_then(Value::as_object);
    for window_key in ["primary_window", "secondary_window"] {
        if let Some(window) = rate_limit.and_then(|limit| limit.get(window_key)) {
            if let Some(part) = codex_window_part(window, tz) {
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

    #[test]
    fn claude_sentence_exact() {
        let body = r#"{
            "five_hour": {"utilization": 42, "resets_at": "2026-07-20T18:30:00+00:00"},
            "seven_day": {"utilization": 81.4, "resets_at": "2026-07-24T10:00:00+00:00"},
            "seven_day_opus": {"utilization": 12}
        }"#;
        assert_eq!(
            claude_usage_sentence(body, &plus2()).unwrap(),
            "Claude usage: session at 42 percent, resets at 20:30, \
             week at 81 percent, resets on 24/07 12:00, opus week at 12 percent"
        );
    }

    #[test]
    fn claude_scoped_weekly_limit_is_reported() {
        // The live shape: dedicated model sections null, the model cap in `limits`.
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
            claude_usage_sentence(body, &plus2()).unwrap(),
            "Claude usage: session at 14 percent, resets at 20:00, \
             week at 40 percent, resets on 23/07 23:00, fable week at 47 percent"
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
            claude_usage_sentence(body, &plus2()).unwrap(),
            "Claude usage: opus week at 12 percent, fable week at 47 percent"
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
                claude_usage_sentence(&body, &plus2()).unwrap(),
                "Claude usage: session at 3 percent",
                "for {limits}"
            );
        }
    }

    #[test]
    fn claude_partial_sections_and_missing_resets() {
        let body = r#"{"five_hour": {"utilization": 0}}"#;
        assert_eq!(
            claude_usage_sentence(body, &plus2()).unwrap(),
            "Claude usage: session at 0 percent"
        );
        let body = r#"{"seven_day": {"utilization": 99.6, "resets_at": null}}"#;
        assert_eq!(
            claude_usage_sentence(body, &plus2()).unwrap(),
            "Claude usage: week at 100 percent"
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
            let error = claude_usage_sentence(body, &plus2()).unwrap_err();
            assert_eq!(error, RealtimeError::NotUnderstood, "for {body:?}");
        }
    }

    #[test]
    fn reset_moment_formats() {
        let tz = plus2();
        let aware = json!("2026-07-20T18:30:00+00:00");
        assert_eq!(format_reset_moment(Some(&aware), false, &tz), Some("20:30".to_string()));
        assert_eq!(format_reset_moment(Some(&aware), true, &tz), Some("20/07 20:30".to_string()));
        let zulu = json!("2026-07-20T23:30:00Z");
        assert_eq!(format_reset_moment(Some(&zulu), true, &tz), Some("21/07 01:30".to_string()));
        // Naive timestamps keep their wall time regardless of timezone.
        let naive = json!("2026-07-20T18:30:00");
        assert_eq!(format_reset_moment(Some(&naive), false, &tz), Some("18:30".to_string()));
        assert_eq!(format_reset_moment(None, false, &tz), None);
        assert_eq!(format_reset_moment(Some(&json!(null)), false, &tz), None);
        assert_eq!(format_reset_moment(Some(&json!(12345)), false, &tz), None);
        assert_eq!(format_reset_moment(Some(&json!("garbage")), false, &tz), None);
    }

    #[test]
    fn epoch_moment_formats() {
        let tz = plus2();
        // 1767225600 == 2026-01-01T00:00:00Z.
        let epoch = json!(1767225600);
        assert_eq!(format_epoch_moment(Some(&epoch), false, &tz), Some("02:00".to_string()));
        assert_eq!(format_epoch_moment(Some(&epoch), true, &tz), Some("01/01 02:00".to_string()));
        // Numeric strings coerce like Python float().
        let text = json!("1767225600");
        assert_eq!(format_epoch_moment(Some(&text), false, &tz), Some("02:00".to_string()));
        assert_eq!(format_epoch_moment(Some(&json!(-5)), false, &tz), None);
        assert_eq!(format_epoch_moment(Some(&json!(1e18)), false, &tz), None);
        assert_eq!(format_epoch_moment(Some(&json!(null)), false, &tz), None);
        assert_eq!(format_epoch_moment(None, false, &tz), None);
    }

    #[test]
    fn openai_sentence_exact() {
        let body = r#"{
            "plan_type": "plus",
            "rate_limit": {
                "primary_window": {
                    "used_percent": 23.4,
                    "limit_window_seconds": 18000,
                    "reset_at": 1767225600
                },
                "secondary_window": {
                    "used_percent": 61.2,
                    "limit_window_seconds": 604800,
                    "reset_at": 1767225600
                }
            }
        }"#;
        assert_eq!(
            openai_usage_sentence(body, &plus2()).unwrap(),
            "OpenAI usage: plus plan, session at 23 percent, resets at 02:00, \
             week at 61 percent, resets on 01/01 02:00"
        );
    }

    #[test]
    fn codex_windows_label_by_duration() {
        let tz = plus2();
        // A 30-day window becomes the month; without reset_at, no reset clause.
        let month = json!({"used_percent": 5, "limit_window_seconds": 2592000});
        assert_eq!(codex_window_part(&month, &tz), Some("month at 5 percent".to_string()));
        // Exactly 24 hours still counts as the session.
        let day = json!({"used_percent": 50, "limit_window_seconds": 86400});
        assert_eq!(codex_window_part(&day, &tz), Some("session at 50 percent".to_string()));
        // Missing/null/zero duration defaults to the session label.
        let no_duration = json!({"used_percent": 7, "limit_window_seconds": null});
        assert_eq!(codex_window_part(&no_duration, &tz), Some("session at 7 percent".to_string()));
        // An unparseable duration falls back to 0 hours, like Python's except.
        let bad_duration = json!({"used_percent": 7, "limit_window_seconds": "abc"});
        assert_eq!(codex_window_part(&bad_duration, &tz), Some("session at 7 percent".to_string()));
        // Windows without used_percent are skipped, not errors.
        assert_eq!(codex_window_part(&json!({"limit_window_seconds": 18000}), &tz), None);
        assert_eq!(codex_window_part(&json!({"used_percent": null}), &tz), None);
        assert_eq!(codex_window_part(&json!({"used_percent": "n/a"}), &tz), None);
        assert_eq!(codex_window_part(&json!("not a window"), &tz), None);
        // A negative epoch drops the reset clause (Windows fromtimestamp).
        let bad_reset = json!({"used_percent": 9, "limit_window_seconds": 18000, "reset_at": -5});
        assert_eq!(codex_window_part(&bad_reset, &tz), Some("session at 9 percent".to_string()));
    }

    #[test]
    fn openai_without_usable_windows_is_not_understood() {
        for body in [
            "{}",
            r#"{"rate_limit": {}}"#,
            r#"{"rate_limit": null, "plan_type": "plus"}"#,
            r#"{"rate_limit": {"primary_window": {"used_percent": null}}}"#,
        ] {
            let error = openai_usage_sentence(body, &plus2()).unwrap_err();
            assert_eq!(error, RealtimeError::NotUnderstood, "for {body:?}");
        }
    }

    #[test]
    fn openai_plan_only_never_stands_alone() {
        // The plan is inserted only when at least one window part exists.
        let body = r#"{"plan_type": "pro", "rate_limit": {"primary_window": {"used_percent": 1}}}"#;
        assert_eq!(
            openai_usage_sentence(body, &plus2()).unwrap(),
            "OpenAI usage: pro plan, session at 1 percent"
        );
    }
}
