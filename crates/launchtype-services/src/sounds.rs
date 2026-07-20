//! UI sound effects — the Rust counterpart of `helpers/sound_player.py`
//! (winsound SND_ASYNC on Windows). Effects live as `<name>.wav` in the
//! app's `sounds/` directory; failures are silent, sounds never block.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

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

    /// Play an arbitrary sound file (timer/alarm custom sounds). Returns
    /// false when the file does not exist or playback failed to start.
    /// Not gated on `enabled`: the Python alert path calls winsound directly,
    /// bypassing the effects toggle, so alerts stay audible in quiet mode.
    pub fn play_alert_file(&self, path: &Path) -> bool {
        if !path.exists() {
            return false;
        }
        play_file(path)
    }

    /// The system beep fallback for alerts without a working custom sound.
    pub fn beep(&self) {
        system_beep();
    }
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
fn system_beep() {
    use windows::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE;
    unsafe {
        let _ = MessageBeep(MESSAGEBOX_STYLE(0xFFFFFFFF));
    }
}

#[cfg(not(windows))]
fn play_file(path: &Path) -> bool {
    // afplay ships with macOS; detach and let it finish on its own.
    std::process::Command::new("afplay")
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(not(windows))]
fn system_beep() {
    let _ = std::process::Command::new("osascript").args(["-e", "beep"]).spawn();
}
