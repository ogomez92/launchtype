//! "Does it work?" mode (`"`) — the UI half of [`launchtype_services::check`].
//!
//! Enter checks what is in the input field, or with the field empty, the row
//! the list is on. The answer comes back on its own time, announced with a
//! sound, and is added to the top of the list.

use std::sync::Arc;

use launchtype_core::check::{classify, Checked, Outcome};
use launchtype_core::i18n::tr;
use launchtype_core::mode::UiMode;
use launchtype_services::sounds::SoundPlayer;
use wxdragon::WxWidget;

use crate::shell::{update_list, with_shell, SharedShell};
use crate::speech::speak_now;

/// How many past checks the list keeps. They last until the app closes, and
/// nobody arrows back through more than this.
const MAX_ROWS: usize = 50;

fn announce(sounds: &SoundPlayer, outcome: &Outcome) {
    sounds.play(if outcome.works { "match" } else { "error" });
    speak_now(&outcome.sentence, true);
}

pub fn run(shell: &SharedShell) {
    let (typed, selected, sounds) = {
        let s = shell.borrow();
        let selected = s
            .list
            .get_selection()
            .and_then(|index| s.items.get(index as usize))
            .map(|item| item.id.clone());
        (s.edit.get_value().trim().to_string(), selected, s.sounds.clone())
    };
    let text = match (typed.is_empty(), selected) {
        (false, _) => typed,
        (true, Some(row)) => row,
        (true, None) => {
            sounds.play("error");
            speak_now(&tr("Paste a link, an email address or a file path first"), true);
            return;
        }
    };
    let target = match classify(&text) {
        Ok(target) => target,
        Err(invalid) => {
            // Left in the field, to be corrected.
            sounds.play("error");
            speak_now(&invalid.message(), true);
            return;
        }
    };

    sounds.play("run");
    speak_now(&tr("Checking"), true);
    // Emptied now, so the next paste does not land on the end of this one.
    shell.borrow().edit.change_value("");
    std::thread::spawn(move || {
        let outcome = launchtype_services::check::run(&target);
        wxdragon::call_after(Box::new(move || finish(sounds, text, outcome)));
    });
}

/// Announce the answer wherever the user is now, since they asked for it, and
/// add it to the list.
fn finish(sounds: Arc<SoundPlayer>, text: String, outcome: Outcome) {
    announce(&sounds, &outcome);
    with_shell(|shell| {
        // Already borrowed means a modal dialog is up, and those run their own
        // event loop, so this can land inside one. The answer has been spoken;
        // only its row is lost.
        let showing = {
            let Ok(mut s) = shell.try_borrow_mut() else { return };
            let checks = &mut s.controller.checks;
            checks.retain(|row| row.text != text);
            checks.insert(0, Checked { text, outcome });
            checks.truncate(MAX_ROWS);
            s.mode == UiMode::Check && s.frame.is_shown()
        };
        if showing {
            update_list(shell);
        }
    });
}
