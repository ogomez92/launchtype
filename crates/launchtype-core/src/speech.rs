//! Speech abstraction. The app provides a Prism-backed implementation on the
//! UI thread; tests use [`RecordingSpeaker`] to assert announcement text.

use std::sync::Mutex;

pub trait Speaker: Send + Sync {
    fn speak(&self, text: &str, interrupt: bool);
}

/// Speaker that discards everything (used when speech init fails; the Python
/// app degrades the same way).
pub struct NullSpeaker;

impl Speaker for NullSpeaker {
    fn speak(&self, _text: &str, _interrupt: bool) {}
}

/// Test double that records every utterance.
#[derive(Default)]
pub struct RecordingSpeaker {
    utterances: Mutex<Vec<(String, bool)>>,
}

impl RecordingSpeaker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn utterances(&self) -> Vec<(String, bool)> {
        self.utterances.lock().unwrap().clone()
    }

    pub fn texts(&self) -> Vec<String> {
        self.utterances.lock().unwrap().iter().map(|(t, _)| t.clone()).collect()
    }
}

impl Speaker for RecordingSpeaker {
    fn speak(&self, text: &str, interrupt: bool) {
        self.utterances.lock().unwrap().push((text.to_string(), interrupt));
    }
}
