//! Prism speech pinned to the wx main thread. Background threads speak
//! through [`UiSpeaker`], which posts to the UI thread via
//! `wxdragon::call_after` (the `wx.CallAfter` of this app).

use std::cell::RefCell;
use std::sync::Arc;

use launchtype_core::speech::Speaker;

thread_local! {
    static SPEECH: RefCell<Option<prism::Speech>> = const { RefCell::new(None) };
}

/// Initialize prism on the current (main) thread. Failure is non-fatal:
/// the app runs silent, exactly like the Python fallback.
pub fn init_speech() {
    match prism::Speech::new() {
        Ok(speech) => {
            log::info!("speech backend: {:?}", speech.backend_name());
            SPEECH.with(|s| *s.borrow_mut() = Some(speech));
        }
        Err(e) => log::warn!("speech unavailable: {e}"),
    }
}

/// Speak immediately; must be called on the main thread.
pub fn speak_now(text: &str, interrupt: bool) {
    SPEECH.with(|s| {
        if let Some(speech) = s.borrow().as_ref() {
            let _ = speech.output(text, interrupt);
        }
    });
}

/// `Speaker` handle usable from any thread.
pub struct UiSpeaker;

impl Speaker for UiSpeaker {
    fn speak(&self, text: &str, interrupt: bool) {
        let text = text.to_string();
        wxdragon::call_after(Box::new(move || speak_now(&text, interrupt)));
    }
}

pub fn shared_speaker() -> Arc<dyn Speaker> {
    Arc::new(UiSpeaker)
}
