//! Timer/alarm alert firing — port of `helpers/alert_notifier.py`:
//! speak "{title}: {description}", play the custom sound or beep.

use std::path::Path;
use std::sync::Arc;

use launchtype_core::speech::Speaker;

use crate::sounds::SoundPlayer;

#[derive(Debug, Clone)]
pub struct AlertItem {
    pub title: String,
    pub description: String,
    pub sound: Option<String>,
}

impl From<&launchtype_core::timers::TimerDef> for AlertItem {
    fn from(t: &launchtype_core::timers::TimerDef) -> Self {
        AlertItem {
            title: t.title.clone(),
            description: t.description.clone(),
            sound: t.sound.clone(),
        }
    }
}

impl From<&launchtype_core::alarms::AlarmDef> for AlertItem {
    fn from(a: &launchtype_core::alarms::AlarmDef) -> Self {
        AlertItem {
            title: a.title.clone(),
            description: a.description.clone(),
            sound: a.sound.clone(),
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
    speaker.speak(&alert_message(item), true);

    if let Some(sound) = item.sound.as_deref() {
        if !sound.is_empty() && sounds.play_alert_file(Path::new(sound)) {
            return;
        }
    }
    // No custom sound (or it failed to play): fall back to the system beep.
    sounds.beep();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_with_and_without_description() {
        let mut item = AlertItem { title: "tea".into(), description: "ready".into(), sound: None };
        assert_eq!(alert_message(&item), "tea: ready");
        item.description.clear();
        assert_eq!(alert_message(&item), "tea");
    }
}
