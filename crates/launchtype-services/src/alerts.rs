//! Timer/alarm alert firing — port of `helpers/alert_notifier.py`:
//! speak "{title}: {description}", play the custom sound or beep.
//!
//! An alarm's sound repeats until the user reaches for the hotkey; a timer's
//! plays once. Either announcement is spoken once, since a screen reader
//! repeating itself would bury whatever the user is listening to. A stopwatch
//! has something new to say each time, so each time it speaks how long it has
//! been running: its title and the time, and not its description.

use std::sync::Arc;

use launchtype_core::speech::Speaker;
use launchtype_core::timers::spoken_duration;

use crate::sounds::SoundPlayer;

#[derive(Debug, Clone)]
pub struct AlertItem {
    pub title: String,
    pub description: String,
    pub sound: Option<String>,
    pub kind: AlertKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    /// An alarm: announced, and its sound loops until the hotkey.
    Alarm,
    /// A countdown that ran out: announced, and its sound plays once.
    Timer,
    /// A stopwatch marking another interval: the time it has been running is
    /// announced, and its sound plays once.
    Tick { elapsed_seconds: i64 },
}

impl AlertItem {
    /// `elapsed_seconds` is how long the timer has been running, which only a
    /// stopwatch has.
    pub fn from_timer(t: &launchtype_core::timers::TimerDef, elapsed_seconds: Option<i64>) -> Self {
        AlertItem {
            title: t.title.clone(),
            description: t.description.clone(),
            sound: t.sound.clone(),
            kind: if t.is_stopwatch() {
                AlertKind::Tick { elapsed_seconds: elapsed_seconds.unwrap_or(0) }
            } else {
                AlertKind::Timer
            },
        }
    }
}

impl From<&launchtype_core::alarms::AlarmDef> for AlertItem {
    fn from(a: &launchtype_core::alarms::AlarmDef) -> Self {
        AlertItem {
            title: a.title.clone(),
            description: a.description.clone(),
            sound: a.sound.clone(),
            kind: AlertKind::Alarm,
        }
    }
}

pub fn alert_message(item: &AlertItem) -> String {
    if item.description.is_empty() {
        item.title.clone()
    } else {
        format!("{}: {}", item.title, item.description)
    }
}

/// What a stopwatch says each time it sounds: "talk: 5 minutes".
fn tick_message(title: &str, elapsed_seconds: i64) -> String {
    let time = spoken_duration(elapsed_seconds);
    if title.is_empty() {
        time
    } else {
        format!("{title}: {time}")
    }
}

pub fn fire_alert(item: &AlertItem, speaker: &Arc<dyn Speaker>, sounds: &SoundPlayer) {
    let sound = item.sound.as_deref().filter(|sound| !sound.is_empty());
    match item.kind {
        AlertKind::Alarm => {
            speaker.speak(&alert_message(item), true);
            if sound.is_some_and(|sound| sounds.play_alert_repeating(sound)) {
                return;
            }
            // No custom sound (or it failed to play): fall back to the system
            // beep, repeating on the same terms the tone would have.
            sounds.beep_repeating();
        }
        AlertKind::Timer => {
            speaker.speak(&alert_message(item), true);
            // An alarm still waiting for the hotkey outranks one pass of a
            // timer's tone, which would cut it off; the announcement is enough.
            if sounds.alarm_looping() {
                return;
            }
            if !sound.is_some_and(|sound| sounds.play_alert_once(sound)) {
                sounds.beep_once();
            }
        }
        AlertKind::Tick { elapsed_seconds } => {
            speaker.speak(&tick_message(&item.title, elapsed_seconds), true);
            // The sound gives way to any alarm or timer still sounding rather
            // than cut it short; the announcement is enough.
            if sounds.alert_sounding() {
                return;
            }
            if !sound.is_some_and(|sound| sounds.play_alert(sound)) {
                sounds.beep_once();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_with_and_without_description() {
        let mut item = AlertItem {
            title: "tea".into(),
            description: "ready".into(),
            sound: None,
            kind: AlertKind::Timer,
        };
        assert_eq!(alert_message(&item), "tea: ready");
        item.description.clear();
        assert_eq!(alert_message(&item), "tea");
    }

    #[test]
    fn tick_says_the_title_and_the_time() {
        assert_eq!(tick_message("talk", 300), "talk: 5 minutes");
        assert_eq!(tick_message("", 90), "1 minute, 30 seconds");
    }
}
