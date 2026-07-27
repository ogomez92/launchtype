//! UI sound effects — the Rust counterpart of `helpers/sound_player.py`
//! (winsound SND_ASYNC on Windows). Effects live as `<name>.wav` in the
//! app's `sounds/` directory; failures are silent, sounds never block.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Subdirectories of `sounds/` holding the alert tones offered in the
/// timer/alarm dialogs. One per alert kind so each dropdown only lists tones
/// written for it — wake-up tones are long and escalating, timer tones short.
pub const ALARM_SOUNDS: &str = "alarms";
pub const TIMER_SOUNDS: &str = "timers";

pub struct SoundPlayer {
    sounds_dir: PathBuf,
    enabled: AtomicBool,
}

impl SoundPlayer {
    pub fn new(sounds_dir: impl Into<PathBuf>, enabled: bool) -> Self {
        SoundPlayer { sounds_dir: sounds_dir.into(), enabled: AtomicBool::new(enabled) }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Play a named effect ("show", "hide", "run", "match", "type", "copy",
    /// "logo") asynchronously. No-op when disabled or the file is missing
    /// (the shipped sounds/ has no type.wav, and PlaySound would substitute
    /// the system default ding on every keystroke otherwise).
    pub fn play(&self, name: &str) {
        if !self.enabled() {
            return;
        }
        let path = self.sounds_dir.join(format!("{name}.wav"));
        if path.is_file() {
            let _ = play_file(&path);
        }
    }

    /// Turn a stored timer/alarm sound into a path. Bundled tones are stored
    /// relative to `sounds/` (`"alarms/dawn.wav"`) so they keep resolving after
    /// a deploy moves the install; a browsed-for file is stored absolute and
    /// passes through untouched.
    pub fn resolve_alert(&self, sound: &str) -> PathBuf {
        let path = Path::new(sound);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.sounds_dir.join(path)
        }
    }

    /// Play a stored timer/alarm sound. Returns false when it does not exist
    /// or playback failed to start.
    /// Not gated on `enabled`: the Python alert path calls winsound directly,
    /// bypassing the effects toggle, so alerts stay audible in quiet mode.
    pub fn play_alert(&self, sound: &str) -> bool {
        let path = self.resolve_alert(sound);
        if !path.exists() {
            return false;
        }
        play_file(&path)
    }

    /// The bundled tones under `sounds/<category>/`, as
    /// `(display name, stored value)` pairs sorted by display name — what the
    /// timer/alarm dialogs put in their sound dropdown. Empty when the folder
    /// is missing, which is all a build without the assets ever sees.
    pub fn bundled_alerts(&self, category: &str) -> Vec<(String, String)> {
        let Ok(entries) = std::fs::read_dir(self.sounds_dir.join(category)) else {
            return Vec::new();
        };
        let mut alerts: Vec<(String, String)> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let ext = path.extension()?.to_str()?.to_ascii_lowercase();
                if ext != "wav" {
                    return None;
                }
                let stem = path.file_stem()?.to_str()?;
                let file = path.file_name()?.to_str()?;
                Some((display_name(stem), format!("{category}/{file}")))
            })
            .collect();
        alerts.sort_by(|a, b| a.0.cmp(&b.0));
        alerts
    }

    /// Cut off whatever is playing. Arrowing through the timer/alarm sound
    /// dropdown previews each tone as it lands, and alarm tones run for
    /// seconds — without this the previews would pile up on each other.
    pub fn stop(&self) {
        stop_playing();
    }

    /// The system beep fallback for alerts without a working custom sound.
    pub fn beep(&self) {
        system_beep();
    }
}

/// `"chime-deluxe"` -> `"Chime Deluxe"`: file stems are the sound's name, so
/// the dropdown can show them once the separators and casing are tidied.
fn display_name(stem: &str) -> String {
    stem.split(['-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(windows)]
fn play_file(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME};

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe { PlaySoundW(PCWSTR(wide.as_ptr()), None, SND_FILENAME | SND_ASYNC).as_bool() }
}

#[cfg(windows)]
fn stop_playing() {
    use windows::core::PCWSTR;
    use windows::Win32::Media::Audio::{PlaySoundW, SND_PURGE};

    // A null name with SND_PURGE stops the sounds this task started.
    let _ = unsafe { PlaySoundW(PCWSTR::null(), None, SND_PURGE) };
}

#[cfg(windows)]
fn system_beep() {
    use windows::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE;
    unsafe {
        let _ = MessageBeep(MESSAGEBOX_STYLE(0xFFFFFFFF));
    }
}

/// Sounds still playing. `spawn` leaves the child for the parent to collect, so
/// without this every effect — one per keystroke in the busiest modes — would
/// strand a zombie for the lifetime of the app.
#[cfg(not(windows))]
static PLAYING: std::sync::Mutex<Vec<std::process::Child>> = std::sync::Mutex::new(Vec::new());

#[cfg(not(windows))]
fn play_file(path: &Path) -> bool {
    // A poisoned lock only means some other thread panicked mid-sweep; the list
    // is still sound, and failing to play here would be worse than continuing.
    let mut playing = PLAYING.lock().unwrap_or_else(|e| e.into_inner());
    // Collect whatever has finished since the last effect. try_wait never
    // blocks, so a sound that is still playing just stays in the list.
    playing.retain_mut(|child| matches!(child.try_wait(), Ok(None)));

    // afplay ships with macOS; let it finish on its own.
    match std::process::Command::new("afplay")
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => {
            playing.push(child);
            true
        }
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn stop_playing() {
    let mut playing = PLAYING.lock().unwrap_or_else(|e| e.into_inner());
    // afplay holds the device for the whole file, so a preview has to be
    // killed outright; `wait` then reaps it instead of leaving a zombie.
    for child in playing.iter_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    playing.clear();
}

#[cfg(not(windows))]
fn system_beep() {
    let _ = std::process::Command::new("osascript").args(["-e", "beep"]).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_stems_become_dropdown_labels() {
        assert_eq!(display_name("dawn"), "Dawn");
        assert_eq!(display_name("chime-deluxe"), "Chime Deluxe");
        assert_eq!(display_name("soft_pop"), "Soft Pop");
    }

    #[test]
    fn bundled_alerts_lists_wavs_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let alarms = dir.path().join(ALARM_SOUNDS);
        std::fs::create_dir_all(&alarms).unwrap();
        for file in ["sunrise.wav", "chime-deluxe.WAV", "notes.txt"] {
            std::fs::write(alarms.join(file), b"").unwrap();
        }
        let player = SoundPlayer::new(dir.path(), true);

        assert_eq!(
            player.bundled_alerts(ALARM_SOUNDS),
            vec![
                ("Chime Deluxe".to_string(), "alarms/chime-deluxe.WAV".to_string()),
                ("Sunrise".to_string(), "alarms/sunrise.wav".to_string()),
            ],
            "wav files only, sorted by label, stored relative to sounds/"
        );
        assert!(
            player.bundled_alerts(TIMER_SOUNDS).is_empty(),
            "a missing category folder is not an error"
        );
    }

    #[test]
    fn bundled_sounds_resolve_under_the_sounds_dir() {
        let player = SoundPlayer::new("C:/app/sounds", true);
        assert_eq!(
            player.resolve_alert("alarms/dawn.wav"),
            Path::new("C:/app/sounds").join("alarms/dawn.wav"),
        );
        // A browsed-for file is stored absolute and must not be re-rooted.
        let absolute = if cfg!(windows) { "C:/music/wake.wav" } else { "/music/wake.wav" };
        assert_eq!(player.resolve_alert(absolute), Path::new(absolute));
    }
}
