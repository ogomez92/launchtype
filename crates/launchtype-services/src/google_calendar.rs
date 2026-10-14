//! Google Calendar client for `%` mode. It talks to the Apps Script the user
//! deployed as a web app (see [`launchtype_core::calendar::script`]), not to
//! Google's Calendar API: the script runs as the user, so there is no Cloud
//! project, OAuth client or token refresh to look after — only the web app's
//! URL and the secret written into the script.
//!
//! Apps Script answers a POST with a redirect to the response body, which
//! ureq follows as a GET, the way the script expects.

use std::time::Duration;

use chrono::{DateTime, Duration as Days, Local, NaiveDate, NaiveTime, TimeZone};
use launchtype_core::calendar::{Event, When, DAYS_AHEAD, EVENT_HOUR};
use launchtype_core::i18n::{format_args, tr, Arg};
use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct CalendarError {
    pub message: String,
    /// The script turned the secret down, so the stored one is no use.
    pub unauthorized: bool,
}

impl CalendarError {
    fn new(message: String) -> Self {
        CalendarError { message, unauthorized: false }
    }
}

/// Send one request to the script and hand back its JSON answer.
fn call(url: &str, token: &str, mut request: Value) -> Result<Value, CalendarError> {
    request["token"] = json!(token);
    let agent = ureq::AgentBuilder::new()
        .timeout(TIMEOUT)
        .user_agent(crate::USER_AGENT)
        .build();
    let response = match agent
        .post(url.trim())
        .set("content-type", "application/json")
        .send_string(&request.to_string())
    {
        Ok(response) => response,
        Err(ureq::Error::Status(code, _)) => {
            return Err(CalendarError::new(format_args(
                &tr("The calendar script answered with status {code}. Check its web app URL."),
                &[("code", Arg::Int(code as i64))],
            )))
        }
        Err(ureq::Error::Transport(t)) => {
            return Err(CalendarError::new(
                tr("Network error: {}").replacen("{}", &t.to_string(), 1),
            ))
        }
    };
    // A deployment that is not open to "Anyone" answers with Google's sign-in
    // page rather than an error status, so anything that is not JSON lands here.
    let body: Value = response
        .into_string()
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or_else(|| {
            CalendarError::new(tr(
                "The calendar script did not answer. Check it is deployed as a web app that executes as you and that anyone can access.",
            ))
        })?;
    match body.get("error").and_then(Value::as_str) {
        None => Ok(body),
        Some("unauthorized") => Err(CalendarError {
            message: tr("The calendar script refused the secret. Set Google Calendar up again."),
            unauthorized: true,
        }),
        Some("not found") => Err(CalendarError::new(tr(
            "That event is no longer on the calendar. It may have been moved or deleted.",
        ))),
        Some(other) => Err(CalendarError::new(other.to_string())),
    }
}

fn parse_event(value: &Value) -> Option<Event> {
    let id = value.get("id")?.as_str()?.to_string();
    let title = value.get("title")?.as_str()?.to_string();
    let start = value.get("start")?.as_str()?.to_string();
    let when = match value.get("date").and_then(Value::as_str) {
        Some(date) => When::AllDay(NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?),
        None => When::At(DateTime::parse_from_rfc3339(&start).ok()?.with_timezone(&Local)),
    };
    Some(Event { id, start_key: start, title, when })
}

/// The events from the start of today to [`DAYS_AHEAD`] days later, soonest
/// first.
pub fn upcoming(url: &str, token: &str) -> Result<Vec<Event>, CalendarError> {
    let today = Local::now().date_naive();
    let from = local_time(today, 0);
    let to = local_time(today + Days::days(DAYS_AHEAD), 0);
    let body = call(
        url,
        token,
        json!({"action": "list", "from": from.to_rfc3339(), "to": to.to_rfc3339()}),
    )?;
    let mut events: Vec<Event> = body
        .get("events")
        .and_then(Value::as_array)
        .map(|events| events.iter().filter_map(parse_event).collect())
        .unwrap_or_default();
    // All-day events go ahead of the timed ones on their day.
    events.sort_by_key(|event| match &event.when {
        When::AllDay(date) => (*date, None),
        When::At(at) => (at.date_naive(), Some(*at)),
    });
    Ok(events)
}

/// Put `title` on the calendar at [`EVENT_HOUR`] on `date`, local time.
pub fn add(url: &str, token: &str, title: &str, date: NaiveDate) -> Result<(), CalendarError> {
    let start = local_time(date, EVENT_HOUR);
    call(url, token, json!({"action": "add", "title": title, "start": start.to_rfc3339()}))?;
    Ok(())
}

/// Rename `event` and move it to `date`. A timed event keeps its time of day
/// and its length; an all-day one stays all-day.
pub fn update(
    url: &str,
    token: &str,
    event: &Event,
    title: &str,
    date: NaiveDate,
) -> Result<(), CalendarError> {
    let mut request =
        json!({"action": "update", "id": event.id, "at": event.start_key, "title": title});
    match &event.when {
        When::AllDay(_) => request["date"] = json!(date.format("%Y-%m-%d").to_string()),
        When::At(at) => request["start"] = json!(at_time(date, at.time()).to_rfc3339()),
    }
    call(url, token, request)?;
    Ok(())
}

/// `hour`:00 on `date` here. A clock change can skip or repeat that hour, in
/// which case the earlier reading (or, failing that, UTC) is near enough.
fn local_time(date: NaiveDate, hour: u32) -> DateTime<Local> {
    at_time(date, NaiveTime::from_hms_opt(hour, 0, 0).unwrap_or_default())
}

/// `time` on `date` here, with the same allowance for clock changes.
fn at_time(date: NaiveDate, time: NaiveTime) -> DateTime<Local> {
    let naive = date.and_time(time);
    Local
        .from_local_datetime(&naive)
        .earliest()
        .unwrap_or_else(|| Local.from_utc_datetime(&naive))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_read_as_timed_or_all_day() {
        let timed = parse_event(
            &json!({"id": "abc", "title": "Dentist", "start": "2026-10-23T06:00:00.000Z"}),
        )
        .unwrap();
        assert_eq!(timed.title, "Dentist");
        assert_eq!(timed.id, "abc");
        assert_eq!(timed.start_key, "2026-10-23T06:00:00.000Z");
        let When::At(at) = timed.when else { panic!("expected a timed event") };
        assert_eq!(at.to_utc().to_rfc3339(), "2026-10-23T06:00:00+00:00");

        let all_day = parse_event(&json!({
            "id": "def",
            "title": "Holiday",
            "start": "2026-10-14T22:00:00.000Z",
            "date": "2026-10-15",
        }))
        .unwrap();
        assert_eq!(all_day.when, When::AllDay(NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()));

        assert_eq!(parse_event(&json!({"id": "x", "title": "No time"})), None);
    }
}
