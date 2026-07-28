//! UI sound effects — the Rust counterpart of `helpers/sound_player.py`
//! (winsound SND_ASYNC on Windows). Effects live as `<name>.wav` in the
//! app's `sounds/` directory; failures are silent, sounds never block.
//!
//! Timer and alarm tones are the exception to "play once": they repeat until
//! the user reaches for the hotkey, so an alert that fires while they are away
//! from the keyboard is still sounding when they get back.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

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
    ///
    /// Effects also stay quiet while an alert is sounding: PlaySound is
    /// per-process, so on Windows one keystroke effect would cut a repeating
    /// alarm off outright, and an alarm outranks a keystroke either way.
    pub fn play(&self, name: &str) {
        if !self.enabled() || alert_repeating() {
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

    /// Play a stored timer/alarm sound once, for auditioning it in the sound
    /// dropdown. Returns false when it does not exist or playback failed to
    /// start.
    /// Not gated on `enabled`: the Python alert path calls winsound directly,
    /// bypassing the effects toggle, so alerts stay audible in quiet mode.
    pub fn play_alert(&self, sound: &str) -> bool {
        let path = self.resolve_alert(sound);
        if !path.exists() {
            return false;
        }
        play_file(&path)
    }

    /// Play a stored timer/alarm sound over and over until `stop_alert`. This
    /// is what a fired timer or alarm uses: one pass of a short tone is easy
    /// to miss from the next room, and the user says they have heard it by
    /// bringing the window up. Returns false when the sound does not exist or
    /// playback failed to start, leaving nothing repeating.
    ///
    /// Only one alert sounds at a time — two timers coming due on the same
    /// tick would otherwise talk over each other, and the second silences the
    /// first the way a stop would.
    pub fn play_alert_repeating(&self, sound: &str) -> bool {
        let path = self.resolve_alert(sound);
        if !path.exists() {
            return false;
        }
        let generation = begin_alert();
        if play_file_repeating(&path, generation) {
            true
        } else {
            end_alert();
            false
        }
    }

    /// The system beep fallback for alerts without a working custom sound,
    /// repeating on the same terms as a tone would.
    pub fn beep_repeating(&self) {
        start_beep_loop(begin_alert());
    }

    /// Cut off whatever is playing. Arrowing through the timer/alarm sound
    /// dropdown previews each tone as it lands, and alarm tones run for
    /// seconds — without this the previews would pile up on each other.
    pub fn stop(&self) {
        // A repeating alert stops here too: the user is at the keyboard
        // working the dropdown, so it has already done its job.
        self.stop_alert();
    }

    /// Silence a repeating timer/alarm alert. The hotkey calls this — reaching
    /// for it is how the user says they have heard the alert.
    pub fn stop_alert(&self) {
        end_alert();
        stop_playing();
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
}

/// Which repeating alert is the current one. Each repeat loop captures the
/// value it started with and gives up as soon as the counter moves on, so
/// starting an alert or stopping one is a single atomic bump — no handles to
/// hold, and nothing for the scheduler thread to join.
static ALERT_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Whether an alert is sounding right now, which the counter alone cannot say.
static ALERT_REPEATING: AtomicBool = AtomicBool::new(false);

/// Gap between beeps when an alert has no usable sound of its own — long
/// enough to read as an alarm rather than as a stuck key.
const BEEP_INTERVAL: Duration = Duration::from_secs(2);

/// Retire whatever was repeating and claim the next generation for the caller.
fn begin_alert() -> u64 {
    ALERT_REPEATING.store(true, Ordering::SeqCst);
    ALERT_GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

fn end_alert() {
    ALERT_REPEATING.store(false, Ordering::SeqCst);
    ALERT_GENERATION.fetch_add(1, Ordering::SeqCst);
}

fn alert_repeating() -> bool {
    ALERT_REPEATING.load(Ordering::SeqCst)
}

/// Whether the loop holding `generation` is still the one that should be
/// making noise.
fn repeat_is_current(generation: u64) -> bool {
    ALERT_GENERATION.load(Ordering::SeqCst) == generation
}

/// Beep every `BEEP_INTERVAL` until the alert is stopped. The generation is
/// checked before each beep, so stopping is silent from that moment on even
/// though the thread is mid-sleep.
fn start_beep_loop(generation: u64) {
    let spawned = std::thread::Builder::new().name("alert-beep".into()).spawn(move || {
        while repeat_is_current(generation) {
            system_beep();
            std::thread::sleep(BEEP_INTERVAL);
        }
    });
    // A thread we could not spawn still owes the user the one beep the alert
    // would have made before any of this.
    if spawned.is_err() {
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

/// Start `path` playing over and over. Returns false when playback could not
/// be started at all, which is the caller's cue to fall back to the beep.
#[cfg(windows)]
fn play_file_repeating(path: &Path, _generation: u64) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_LOOP};

    // SND_LOOP repeats the tone with no gap of its own and runs until the next
    // PlaySound call on this process — which is exactly what the SND_PURGE in
    // `stop_playing` is. No repeat thread needed here.
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        PlaySoundW(PCWSTR(wide.as_ptr()), None, SND_FILENAME | SND_ASYNC | SND_LOOP).as_bool()
    }
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

/// How often the repeat loop looks in on the tone it started: the gap between
/// repetitions, and how long a stop can take to fall silent.
#[cfg(not(windows))]
const REPEAT_POLL: Duration = Duration::from_millis(100);

/// afplay ships with macOS; let it finish on its own.
#[cfg(not(windows))]
fn spawn_player(path: &Path) -> Option<std::process::Child> {
    std::process::Command::new("afplay")
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
}

#[cfg(not(windows))]
fn play_file(path: &Path) -> bool {
    // A poisoned lock only means some other thread panicked mid-sweep; the list
    // is still sound, and failing to play here would be worse than continuing.
    let mut playing = PLAYING.lock().unwrap_or_else(|e| e.into_inner());
    // Collect whatever has finished since the last effect. try_wait never
    // blocks, so a sound that is still playing just stays in the list.
    playing.retain_mut(|child| matches!(child.try_wait(), Ok(None)));

    match spawn_player(path) {
        Some(child) => {
            playing.push(child);
            true
        }
        None => false,
    }
}

/// Start `path` playing over and over. Returns false when playback could not
/// be started at all, which is the caller's cue to fall back to the beep.
#[cfg(not(windows))]
fn play_file_repeating(path: &Path, generation: u64) -> bool {
    // afplay plays the file once and exits, so repeating it takes a thread
    // that starts it again. The thread owns the child rather than parking it
    // in PLAYING: it has to be able to kill the tone mid-play when the alert
    // is stopped, and only one owner can wait on a child.
    let Some(mut child) = spawn_player(path) else {
        return false;
    };
    let path = path.to_path_buf();
    // A thread that will not spawn is not worth failing over: the tone still
    // plays through once, which beats trading an alert that is already
    // sounding for a beep.
    let _ = std::thread::Builder::new().name("alert-repeat".into()).spawn(move || loop {
        if !repeat_is_current(generation) {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        match child.try_wait() {
            // The tone reached its end: round it goes again.
            Ok(Some(_)) => match spawn_player(&path) {
                Some(next) => child = next,
                None => return,
            },
            Ok(None) => std::thread::sleep(REPEAT_POLL),
            Err(_) => return,
        }
    });
    true
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
    // `status`, not `spawn`: an alert with no tone of its own beeps every two
    // seconds until it is dismissed, and a child left unreaped each time would
    // strand a zombie for the lifetime of the app. Beeps only ever happen off
    // the UI thread, so the wait costs nothing visible.
    let _ = std::process::Command::new("osascript").args(["-e", "beep"]).status();
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

    /// One test for the whole repeat state machine: the generation is process
    /// wide, so splitting this up would just let the cases race each other.
    #[test]
    fn a_new_alert_retires_the_one_before_it() {
        assert!(!alert_repeating(), "nothing is sounding before an alert fires");

        let first = begin_alert();
        assert!(alert_repeating());
        assert!(repeat_is_current(first));

        // A second timer coming due takes over: the first loop sees a
        // generation that is no longer its own and gives up.
        let second = begin_alert();
        assert!(!repeat_is_current(first), "the first loop stands down");
        assert!(repeat_is_current(second));

        end_alert();
        assert!(!alert_repeating(), "the hotkey leaves nothing sounding");
        assert!(!repeat_is_current(second), "and no loop still thinks it is current");
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
