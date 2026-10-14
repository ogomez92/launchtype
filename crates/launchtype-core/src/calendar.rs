//! Google Calendar mode (`%`): the parts with no I/O in them — reading the
//! date the user types, and turning an event into the sentence the list
//! shows. The calendar itself is reached through a small Apps Script the user
//! deploys under their own account (see [`script`]), which is what spares
//! them a Google Cloud project and an OAuth client.

use chrono::{DateTime, Datelike, Local, NaiveDate, Weekday};

use crate::i18n::{format_args, tr, Arg};

/// How far ahead the list looks, counting today.
pub const DAYS_AHEAD: i64 = 30;

/// The hour every new event starts at: early enough that the calendar's
/// reminder arrives at the start of the day.
pub const EVENT_HOUR: u32 = 8;

/// The Apps Script source, with the secret still to be filled in.
const SCRIPT: &str = include_str!("../data/google-calendar.gs");

/// The Apps Script to paste into script.google.com, carrying `token` as the
/// secret every request has to present.
pub fn script(token: &str) -> String {
    SCRIPT.replace("__TOKEN__", token)
}

/// When an event happens, the way the calendar reports it.
#[derive(Debug, Clone, PartialEq)]
pub enum When {
    AllDay(NaiveDate),
    At(DateTime<Local>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// The calendar's id for the event. A repeating event's occurrences all
    /// share one, so it is `start_key` that tells them apart.
    pub id: String,
    /// The instant the script said the event starts at, exactly as it said
    /// it: handed back with the id, it is how the script finds the event again
    /// to change it.
    pub start_key: String,
    pub title: String,
    pub when: When,
}

impl Event {
    pub fn date(&self) -> NaiveDate {
        match &self.when {
            When::AllDay(date) => *date,
            When::At(at) => at.date_naive(),
        }
    }
}

/// Read a typed date. The year always leads; `day_first` says whether the
/// day comes before the month (the European order, `20150110` for 1 October)
/// or after it (the US one, `20251001`). In the US order `20260925` is a full
/// date, `0925` (or `925`) a month and day of this year, and `25` (or `5`) a
/// day of this month; the European order swaps the day and the month in the
/// first two. Anything that is not all digits, or names a day the calendar
/// does not have, is `None`.
pub fn parse_date(input: &str, today: NaiveDate, day_first: bool) -> Option<NaiveDate> {
    let digits = input.trim();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let number = |range: std::ops::Range<usize>| digits[range].parse::<u32>().ok();
    let (year, first, second) = match digits.len() {
        1 | 2 => {
            let day = number(0..digits.len())?;
            return NaiveDate::from_ymd_opt(today.year(), today.month(), day);
        }
        3 => (today.year(), number(0..1)?, number(1..3)?),
        4 => (today.year(), number(0..2)?, number(2..4)?),
        8 => (number(0..4)? as i32, number(4..6)?, number(6..8)?),
        _ => return None,
    };
    let (month, day) = if day_first { (second, first) } else { (first, second) };
    NaiveDate::from_ymd_opt(year, month, day)
}

/// `date` written the way [`parse_date`] reads a full one.
pub fn digits(date: NaiveDate, day_first: bool) -> String {
    if day_first {
        format!("{:04}{:02}{:02}", date.year(), date.day(), date.month())
    } else {
        format!("{:04}{:02}{:02}", date.year(), date.month(), date.day())
    }
}

/// How to type a day, said out loud: what the short and full forms look like
/// in the order the user has chosen, using 25 September of this year.
pub fn date_hint(today: NaiveDate, day_first: bool) -> String {
    let example = NaiveDate::from_ymd_opt(today.year(), 9, 25).unwrap_or(today);
    let full = digits(example, day_first);
    let pair = if day_first { tr("a day and month") } else { tr("a month and day") };
    format_args(
        &tr("a day like 25, {pair} like {short}, or a full date like {full}"),
        &[("pair", Arg::Str(&pair)), ("short", Arg::Str(&full[4..])), ("full", Arg::Str(&full))],
    )
}

fn weekday_name(weekday: Weekday) -> String {
    match weekday {
        Weekday::Mon => tr("Monday"),
        Weekday::Tue => tr("Tuesday"),
        Weekday::Wed => tr("Wednesday"),
        Weekday::Thu => tr("Thursday"),
        Weekday::Fri => tr("Friday"),
        Weekday::Sat => tr("Saturday"),
        Weekday::Sun => tr("Sunday"),
    }
}

fn month_name(month: u32) -> String {
    match month {
        1 => tr("January"),
        2 => tr("February"),
        3 => tr("March"),
        4 => tr("April"),
        5 => tr("May"),
        6 => tr("June"),
        7 => tr("July"),
        8 => tr("August"),
        9 => tr("September"),
        10 => tr("October"),
        11 => tr("November"),
        12 => tr("December"),
        _ => String::new(),
    }
}

/// A day the way it is said: "today", "tomorrow", or "Friday 25 September",
/// with the year only when it is not this one.
pub fn day_label(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        return tr("today");
    }
    if Some(date) == today.succ_opt() {
        return tr("tomorrow");
    }
    let day = format_args(
        &tr("{weekday} {day} {month}"),
        &[
            ("weekday", Arg::Str(&weekday_name(date.weekday()))),
            ("day", Arg::Int(date.day() as i64)),
            ("month", Arg::Str(&month_name(date.month()))),
        ],
    );
    if date.year() == today.year() {
        day
    } else {
        format!("{day} {}", date.year())
    }
}

/// One row of the list: the day first, since that is what the list is in
/// order of, then the time for events that have one, then the title.
pub fn event_label(event: &Event, today: NaiveDate) -> String {
    let day = day_label(event.date(), today);
    match &event.when {
        When::AllDay(_) => format!("{day}, {}", event.title),
        When::At(at) => format!("{day}, {}, {}", at.format("%H:%M"), event.title),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn today() -> NaiveDate {
        date(2026, 10, 14)
    }

    fn us(input: &str) -> Option<NaiveDate> {
        parse_date(input, today(), false)
    }

    fn eu(input: &str) -> Option<NaiveDate> {
        parse_date(input, today(), true)
    }

    #[test]
    fn eight_digits_are_a_full_date() {
        assert_eq!(us("20270925"), Some(date(2027, 9, 25)));
        assert_eq!(us("20251001"), Some(date(2025, 10, 1)));
        assert_eq!(eu("20150110"), Some(date(2015, 10, 1)));
        assert_eq!(eu("20272509"), Some(date(2027, 9, 25)));
    }

    #[test]
    fn four_or_three_digits_are_month_and_day_of_this_year() {
        assert_eq!(us("1225"), Some(date(2026, 12, 25)));
        assert_eq!(us("0925"), Some(date(2026, 9, 25)));
        assert_eq!(us("925"), Some(date(2026, 9, 25)));
        assert_eq!(eu("2512"), Some(date(2026, 12, 25)));
        assert_eq!(eu("0109"), Some(date(2026, 9, 1)));
        assert_eq!(eu("109"), Some(date(2026, 9, 1)));
    }

    #[test]
    fn one_or_two_digits_are_a_day_of_this_month() {
        assert_eq!(us("25"), Some(date(2026, 10, 25)));
        assert_eq!(us("5"), Some(date(2026, 10, 5)));
        assert_eq!(us(" 05 "), Some(date(2026, 10, 5)));
        assert_eq!(eu("25"), Some(date(2026, 10, 25)));
    }

    #[test]
    fn days_the_calendar_does_not_have_are_refused() {
        assert_eq!(us("32"), None);
        assert_eq!(us("0231"), None);
        assert_eq!(us("1301"), None);
        assert_eq!(us("0"), None);
        assert_eq!(us("20270229"), None);
        assert_eq!(eu("0925"), None);
        assert_eq!(eu("20260925"), None);
    }

    #[test]
    fn anything_but_those_lengths_of_digits_is_refused() {
        assert_eq!(us(""), None);
        assert_eq!(us("25/09"), None);
        assert_eq!(us("tomorrow"), None);
        assert_eq!(us("260925"), None);
        assert_eq!(us("12345"), None);
    }

    #[test]
    fn full_dates_are_written_back_in_the_chosen_order() {
        let day = date(2026, 9, 5);
        assert_eq!(digits(day, false), "20260905");
        assert_eq!(digits(day, true), "20260509");
        for day_first in [false, true] {
            assert_eq!(parse_date(&digits(day, day_first), today(), day_first), Some(day));
        }
    }

    #[test]
    fn the_hint_shows_the_chosen_order() {
        assert_eq!(
            date_hint(today(), false),
            "a day like 25, a month and day like 0925, or a full date like 20260925"
        );
        assert_eq!(
            date_hint(today(), true),
            "a day like 25, a day and month like 2509, or a full date like 20262509"
        );
    }

    #[test]
    fn near_days_are_named_and_far_ones_spelled_out() {
        assert_eq!(day_label(today(), today()), "today");
        assert_eq!(day_label(date(2026, 10, 15), today()), "tomorrow");
        assert_eq!(day_label(date(2026, 10, 23), today()), "Friday 23 October");
        assert_eq!(day_label(date(2027, 1, 4), today()), "Monday 4 January 2027");
    }

    #[test]
    fn event_labels_carry_the_time_only_when_there_is_one() {
        let timed = Event {
            id: String::new(),
            start_key: String::new(),
            title: "Dentist".into(),
            when: When::At(Local.with_ymd_and_hms(2026, 10, 23, 8, 0, 0).unwrap()),
        };
        assert_eq!(event_label(&timed, today()), "Friday 23 October, 08:00, Dentist");
        let all_day = Event {
            id: String::new(),
            start_key: String::new(),
            title: "Holiday".into(),
            when: When::AllDay(date(2026, 10, 15)),
        };
        assert_eq!(event_label(&all_day, today()), "tomorrow, Holiday");
    }

    #[test]
    fn the_script_carries_the_token() {
        let script = script("s3cret");
        assert!(script.contains("const TOKEN = 's3cret';"));
        assert!(!script.contains("__TOKEN__"));
    }
}
