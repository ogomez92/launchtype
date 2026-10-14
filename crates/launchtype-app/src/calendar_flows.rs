//! Google Calendar mode (`%`) — the UI half of
//! [`launchtype_services::google_calendar`].
//!
//! Entering the mode fetches the next month of events in the background; the
//! first time, it sets up the Apps Script the calendar is reached through.
//! Adding an event (the Add button) or changing one (Edit) asks two questions
//! in the input field, the way a command's `{{query}}` does: the title, then
//! the day. Every new event starts at 8:00, so the calendar's own reminder is
//! what does the notifying.

use launchtype_core::calendar::{self, Event, DAYS_AHEAD};
use launchtype_core::i18n::{format_args, tr, Arg};
use launchtype_core::mode::UiMode;
use launchtype_core::snippet_vars::Ask;
use launchtype_services::{clipboard, google_calendar};
use wxdragon::WxWidget;

use crate::controller::{CalendarState, Item, ItemKind};
use crate::shell::{report_error, start_query, update_list, with_shell, with_window_up, SharedShell};
use crate::speech::speak_now;

/// Called right after `%` switches the mode: fetch the events, setting the
/// script up first if there is none yet. Cancelling the setup leaves the
/// mode's "Set up Google Calendar" row there to try again.
pub fn enter_calendar_mode(shell: &SharedShell) {
    if !shell.borrow().settings.settings.google_calendar_configured() {
        shell.borrow_mut().controller.calendar = CalendarState::NotSetUp;
        update_list(shell);
        if !set_up(shell) {
            return;
        }
    }
    refresh(shell, true);
}

/// Enter on one of the mode's action rows.
pub fn run_action(shell: &SharedShell, action: &str) {
    match action {
        "setup" if set_up(shell) => refresh(shell, true),
        "refresh" => refresh(shell, true),
        "loading" => speak_now(&tr("Still loading the calendar"), true),
        _ => {}
    }
}

/// Copy the script to the clipboard and ask for the URL Google gives it once
/// deployed. The secret is kept across a second setup, so a script that is
/// already deployed goes on working when only the URL is being corrected.
fn set_up(shell: &SharedShell) -> bool {
    let (url, mut token) = {
        let s = shell.borrow();
        let settings = &s.settings.settings;
        (settings.google_calendar_url.clone(), settings.google_calendar_token.clone())
    };
    if token.is_empty() {
        token = uuid::Uuid::new_v4().simple().to_string();
    }
    let script = calendar::script(&token);
    clipboard::set_text(&script);
    let Some(url) = with_window_up(shell, |frame| {
        crate::dialogs::google_calendar_dialog(frame, &url, &script)
    }) else {
        return false;
    };
    let mut s = shell.borrow_mut();
    s.settings.settings.google_calendar_url = url;
    s.settings.settings.google_calendar_token = token;
    let _ = s.settings.save();
    true
}

/// Fetch the events again on a background thread. The list says it is loading
/// meanwhile, and the result is announced when it lands (if `announce`) —
/// unless the user has moved on to another mode, in which case it is only
/// kept for next time.
fn refresh(shell: &SharedShell, announce: bool) {
    let (url, token) = {
        let mut s = shell.borrow_mut();
        s.controller.calendar = CalendarState::Loading;
        let settings = &s.settings.settings;
        (settings.google_calendar_url.clone(), settings.google_calendar_token.clone())
    };
    update_list(shell);
    std::thread::spawn(move || {
        let result = google_calendar::upcoming(&url, &token);
        wxdragon::call_after(Box::new(move || {
            with_shell(|shell| {
                let announcement = match &result {
                    Ok(events) => format_args(
                        &tr("{count} events in the next {days} days"),
                        &[("count", Arg::Int(events.len() as i64)), ("days", Arg::Int(DAYS_AHEAD))],
                    ),
                    Err(error) => error.message.clone(),
                };
                let showing = {
                    let mut s = shell.borrow_mut();
                    s.controller.calendar = match result {
                        Ok(events) => CalendarState::Loaded(events),
                        Err(error) => CalendarState::Failed(error.message),
                    };
                    s.mode == UiMode::GoogleCalendar && s.frame.is_shown()
                };
                // A title or date being typed owns the input field; the list
                // catches up once it is answered.
                if !showing || shell.borrow().pending_query.is_some() {
                    return;
                }
                update_list(shell);
                if announce {
                    speak_now(&announcement, true);
                }
            });
        }));
    });
}


/// Ask for the new event's title, then its day (see [`prompt`]).
pub fn start_adding(shell: &SharedShell) {
    if !shell.borrow().settings.settings.google_calendar_configured() && !set_up(shell) {
        return;
    }
    let item = Item {
        name: tr("Add an event"),
        shortcut: String::new(),
        id: String::new(),
        kind: ItemKind::CalendarAction { action: "add" },
    };
    start_query(shell, item, vec![Ask::Positional; 2]);
}

/// Ask for `event`'s title and day again, each starting out as what it is
/// now, so Enter alone keeps it.
pub fn start_editing(shell: &SharedShell, event: Event) {
    let item = Item {
        name: event.title.clone(),
        shortcut: String::new(),
        id: String::new(),
        kind: ItemKind::CalendarEvent { event },
    };
    start_query(shell, item, vec![Ask::Positional; 2]);
}

/// Whether a pending query is one of this mode's: adding an event, or
/// changing one.
pub fn is_calendar_query(kind: &ItemKind) -> bool {
    matches!(kind, ItemKind::CalendarAction { .. } | ItemKind::CalendarEvent { .. })
}

/// What the input field asks for while an event is being added or changed.
pub fn prompt(kind: &ItemKind, index: usize, day_first: bool) -> String {
    let hint = calendar::date_hint(today(), day_first);
    match (kind, index) {
        (ItemKind::CalendarEvent { event }, 0) => format_args(
            &tr("New title, now {title}"),
            &[("title", Arg::Str(&event.title))],
        ),
        (ItemKind::CalendarEvent { event }, _) => format_args(
            &tr("New day, now {day}: {hint}"),
            &[
                ("day", Arg::Str(&calendar::day_label(event.date(), today()))),
                ("hint", Arg::Str(&hint)),
            ],
        ),
        (_, 0) => tr("Event title"),
        (_, _) => format_args(&tr("Event day: {hint}"), &[("hint", Arg::Str(&hint))]),
    }
}

/// What the field starts out holding for question `index`: an event being
/// changed offers what it has now, selected so that typing replaces it.
pub fn prefill(kind: &ItemKind, index: usize, day_first: bool) -> Option<String> {
    let ItemKind::CalendarEvent { event } = kind else { return None };
    Some(match index {
        0 => event.title.clone(),
        _ => calendar::digits(event.date(), day_first),
    })
}

/// Why `answer` will not do for question `index`, or `None` if it will.
pub fn check_answer(index: usize, answer: &str, day_first: bool) -> Option<String> {
    match index {
        0 if answer.trim().is_empty() => Some(tr("The event needs a title")),
        1 if calendar::parse_date(answer, today(), day_first).is_none() => Some(format_args(
            &tr("That is not a day. Type {hint}"),
            &[("hint", Arg::Str(&calendar::date_hint(today(), day_first)))],
        )),
        _ => None,
    }
}

/// The day answer `index` names, said out loud, if it is a day that has
/// already gone by — an event there would never sound its reminder.
pub fn past_day(index: usize, answer: &str, day_first: bool) -> Option<String> {
    if index != 1 {
        return None;
    }
    let today = today();
    let date = calendar::parse_date(answer, today, day_first)?;
    (date < today).then(|| calendar::day_label(date, today))
}

/// Ask whether to keep a day that has already passed. Declining leaves the
/// typed day in the field, selected so typing replaces it, and asks again.
pub fn confirm_past_day(shell: &SharedShell, day: &str, prompt: &str) -> bool {
    if with_window_up(shell, |frame| crate::dialogs::confirm_past_event_day(frame, day)) {
        return true;
    }
    let s = shell.borrow();
    s.edit.set_focus();
    s.edit.select_all();
    speak_now(prompt, true);
    false
}

fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// Both questions are answered: add the event, or change the one being
/// edited, then fetch the list again so it shows.
pub fn finish(shell: &SharedShell, kind: &ItemKind, answers: &[String]) {
    let [title, day] = answers else { return };
    let title = title.trim().to_string();
    let (url, token, day_first, sounds) = {
        let s = shell.borrow();
        let settings = &s.settings.settings;
        (
            settings.google_calendar_url.clone(),
            settings.google_calendar_token.clone(),
            settings.day_first(),
            s.sounds.clone(),
        )
    };
    let Some(date) = calendar::parse_date(day, today(), day_first) else { return };
    let editing = match kind {
        ItemKind::CalendarEvent { event } => Some(event.clone()),
        _ => None,
    };
    sounds.play("run");
    speak_now(
        &if editing.is_some() { tr("Changing the event") } else { tr("Adding the event") },
        true,
    );
    std::thread::spawn(move || {
        let result = match &editing {
            Some(event) => google_calendar::update(&url, &token, event, &title, date),
            None => google_calendar::add(&url, &token, &title, date),
        };
        wxdragon::call_after(Box::new(move || {
            with_shell(|shell| match result {
                Ok(()) => {
                    sounds.play("match");
                    let done = if editing.is_some() {
                        tr("{title} is now on {day}")
                    } else {
                        tr("{title} added for {day}")
                    };
                    speak_now(
                        &format_args(
                            &done,
                            &[
                                ("title", Arg::Str(&title)),
                                ("day", Arg::Str(&calendar::day_label(date, today()))),
                            ],
                        ),
                        true,
                    );
                    // Quietly: the sentence above is the one worth hearing.
                    if shell.borrow().mode == UiMode::GoogleCalendar {
                        refresh(shell, false);
                    }
                }
                Err(error) => {
                    sounds.play("error");
                    let title = if editing.is_some() {
                        tr("Event not changed")
                    } else {
                        tr("Event not added")
                    };
                    report_error(shell, &title, &error.message);
                }
            });
        }));
    });
}
