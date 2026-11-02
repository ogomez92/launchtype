//! Timer/alarm alert firing — port of `helpers/alert_notifier.py`:
//! speak "{title}: {description}", play the custom sound or beep.
//!
//! An alarm's sound repeats until the user reaches for the hotkey; a timer's
//! plays once. Either announcement is spoken once, since a screen reader
//! repeating itself would bury whatever the user is listening to. A stopwatch
//! only sounds: it may go off every second, and has nothing new to say.

use std::sync::Arc;

use launchtype_core::speech::Speaker;

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
    /// A stopwatch marking another interval: its sound plays once, unspoken.
    Tick,
}

impl From<&launchtype_core::timers::TimerDef> for AlertItem {
    fn from(t: &launchtype_core::timers::TimerDef) -> Self {
        AlertItem {
            title: t.title.clone(),
            description: t.description.clone(),
            sound: t.sound.clone(),
            kind: if t.is_stopwatch() { AlertKind::Tick } else { AlertKind::Timer },
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
        AlertKind::Tick => {
            // A tick gives way to any alarm or timer still sounding rather
            // than cut it short.
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
}
