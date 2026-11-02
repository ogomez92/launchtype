//! UI sound effects — the Rust counterpart of `helpers/sound_player.py`
//! (winsound SND_ASYNC on Windows). Effects live as `<name>.wav` in the
//! app's `sounds/` directory; failures are silent, sounds never block.
//!
//! Alarm tones are the exception to "play once": they repeat until the user
//! reaches for the hotkey, so an alarm that goes off while they are away from
//! the keyboard is still sounding when they get back. Timer and stopwatch
//! tones play once, like any other effect.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Subdirectories of `sounds/` holding the alert tones offered in the
/// timer/alarm dialogs. One per alert kind so each dropdown only lists tones
/// written for it — wake-up tones are long and escalating, timer tones short.
pub const ALARM_SOUNDS: &str = "alarms";
pub const TIMER_SOUNDS: &str = "timers";
/// Short ticks and markers: a stopwatch can sound every second.
pub const STOPWATCH_SOUNDS: &str = "stopwatch";

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
        if !self.enabled() || alert_sounding() {
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

    /// Play a fired timer's sound once. Until it has played through it counts
    /// as a sounding alert: the hotkey silences it instead of bringing the
    /// window up, and effects and stopwatch ticks hold off rather than cut it
    /// short. Returns false when the sound does not exist or playback failed
    /// to start.
    pub fn play_alert_once(&self, sound: &str) -> bool {
        let path = self.resolve_alert(sound);
        if !path.exists() || !play_file(&path) {
            return false;
        }
        let length = wav_duration(&path).unwrap_or(UNKNOWN_ALERT_LENGTH);
        *ALERT_UNTIL.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now() + length);
        true
    }

    /// Play a stored alarm sound over and over until `stop_alert`: one pass of
    /// a short tone is easy to miss from the next room, and the user says they
    /// have heard it with the hotkey. Returns false when the sound does not
    /// exist or playback failed to start, leaving nothing repeating.
    ///
    /// Only one alarm sounds at a time — two coming due on the same tick
    /// would otherwise talk over each other, and the second silences the
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

    /// The system beep fallback for alarms without a working custom sound,
    /// repeating on the same terms as a tone would.
    pub fn beep_repeating(&self) {
        start_beep_loop(begin_alert());
    }

    /// One system beep, the fallback for timers and stopwatches without a
    /// working sound. Off the calling thread: on macOS the beep waits for
    /// osascript, and the scheduler has other timers to keep to.
    pub fn beep_once(&self) {
        if std::thread::Builder::new().name("alert-beep".into()).spawn(system_beep).is_err() {
            system_beep();
        }
    }

    /// Whether an alarm is looping or a timer's sound is still playing.
    /// Stopwatch ticks do not count: one sounding every second would leave the
    /// hotkey nothing to do but silence it.
    pub fn alert_sounding(&self) -> bool {
        alert_sounding()
    }

    /// Whether an alarm is looping, which a timer's one pass gives way to.
    pub fn alarm_looping(&self) -> bool {
        alert_repeating()
    }

    /// Cut off whatever is playing. Arrowing through the timer/alarm sound
    /// dropdown previews each tone as it lands, and alarm tones run for
    /// seconds — without this the previews would pile up on each other.
    pub fn stop(&self) {
        // A repeating alert stops here too: the user is at the keyboard
        // working the dropdown, so it has already done its job.
        self.stop_alert();
    }

    /// Silence a sounding alarm or timer. The hotkey calls this — reaching for
    /// it is how the user says they have heard the alert. Returns whether
    /// there was one to silence.
    pub fn stop_alert(&self) -> bool {
        let was_sounding = alert_sounding();
        end_alert();
        stop_playing();
        was_sounding
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

/// When the one pass of a fired timer's sound ends. Not an alert generation:
/// nothing repeats, so there is no loop to retire, only a moment to wait out.
static ALERT_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// How long a timer's sound is taken to play when its length cannot be read
/// from the file.
const UNKNOWN_ALERT_LENGTH: Duration = Duration::from_secs(3);

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
    *ALERT_UNTIL.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn alert_repeating() -> bool {
    ALERT_REPEATING.load(Ordering::SeqCst)
}

fn alert_sounding() -> bool {
    alert_repeating()
        || ALERT_UNTIL
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some_and(|until| Instant::now() < until)
}

/// How long a WAV file plays, from its header: the data chunk's size over the
/// format chunk's byte rate. None for anything that is not a readable WAV.
fn wav_duration(path: &Path) -> Option<Duration> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    let mut riff = [0u8; 12];
    file.read_exact(&mut riff).ok()?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return None;
    }
    let mut byte_rate = None;
    loop {
        let mut header = [0u8; 8];
        file.read_exact(&mut header).ok()?;
        let size = u32::from_le_bytes(header[4..8].try_into().ok()?);
        // Chunks are padded to an even length.
        let mut skip = i64::from(size) + i64::from(size & 1);
        match &header[0..4] {
            b"fmt " if size >= 12 => {
                // format tag (2), channels (2), sample rate (4), byte rate (4)
                let mut fmt = [0u8; 12];
                file.read_exact(&mut fmt).ok()?;
                byte_rate = Some(u32::from_le_bytes(fmt[8..12].try_into().ok()?));
                skip -= 12;
            }
            b"data" => {
                let rate = byte_rate.filter(|rate| *rate > 0)?;
                return Some(Duration::from_secs_f64(f64::from(size) / f64::from(rate)));
            }
            _ => {}
        }
        file.seek(SeekFrom::Current(skip)).ok()?;
    }
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

    /// Every effect the app asks for by name, and whether `assets/sounds` is
    /// expected to hold a file for it.
    ///
    /// `play` no-ops on a missing file, on purpose — `type` has never shipped
    /// a sound, and PlaySound would substitute the system ding on every
    /// keystroke if it were asked for one that is not there. That silence is
    /// also what a misspelt effect name looks like, which is what this table
    /// is for: a caller playing `"askqery"` fails as quietly as one playing
    /// `"type"` does deliberately, and only this test tells them apart.
    const EFFECTS: [(&str, bool); 10] = [
        ("askquery", true),
        ("copy", true),
        ("error", true),
        ("hide", true),
        ("logo", true),
        ("match", true),
        ("run", true),
        ("show", true),
        ("type", false),
        ("typequery", true),
    ];

    #[test]
    fn every_effect_name_the_app_plays_has_a_shipped_sound() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds");
        for (name, shipped) in EFFECTS {
            let file = assets.join(format!("{name}.wav"));
            assert_eq!(
                file.is_file(),
                shipped,
                "{}: assets/sounds/{name}.wav",
                if shipped { "missing" } else { "unexpectedly present" }
            );
        }
    }

    /// The other half of the same guard: a sound nobody plays is dead weight
    /// in every build, and usually means a caller was renamed and the file was
    /// not (or the other way round).
    #[test]
    fn every_shipped_sound_is_played_by_something() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds");
        for entry in std::fs::read_dir(&assets).expect("assets/sounds").flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| !ext.eq_ignore_ascii_case("wav")) {
                continue;
            }
            let stem = path.file_stem().unwrap().to_string_lossy().to_string();
            // Timer and alarm tones live in subfolders and are chosen by the
            // user, not played by name; `timer.wav` is the legacy default.
            if stem == "timer" {
                continue;
            }
            assert!(
                EFFECTS.iter().any(|(name, _)| *name == stem),
                "assets/sounds/{stem}.wav is never played by name"
            );
        }
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
    fn wav_duration_reads_the_header() {
        let dir = tempfile::tempdir().unwrap();
        // Half a second of 8 kHz 16-bit mono, behind an odd-sized chunk the
        // reader has to step over, padding byte included.
        let data_len: u32 = 8000;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(4 + 14 + 24 + 8 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"LIST");
        wav.extend_from_slice(&5u32.to_le_bytes());
        wav.extend_from_slice(b"hello\0");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend(std::iter::repeat_n(0u8, data_len as usize));
        let path = dir.path().join("half.wav");
        std::fs::write(&path, &wav).unwrap();
        assert_eq!(wav_duration(&path), Some(Duration::from_millis(500)));

        let not_wav = dir.path().join("song.mp3");
        std::fs::write(&not_wav, b"ID3 not a wav").unwrap();
        assert_eq!(wav_duration(&not_wav), None);
    }

    /// A stopwatch can sound every second, and on Windows each tick cuts the
    /// one before it off: a long tone there would never be heard whole.
    #[test]
    fn every_stopwatch_sound_is_short() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds");
        let player = SoundPlayer::new(&assets, true);
        let sounds = player.bundled_alerts(STOPWATCH_SOUNDS);
        assert_eq!(sounds.len(), 25);
        for (name, stored) in sounds {
            let length = wav_duration(&player.resolve_alert(&stored)).expect(&name);
            assert!(length < Duration::from_millis(1500), "{name} plays for {length:?}");
        }
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
