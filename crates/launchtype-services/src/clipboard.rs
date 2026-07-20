//! Thin clipboard access helpers over clipboard-rs (replaces pyperclip and
//! the hand-rolled CF_HDROP code). A fresh context per call keeps the API
//! simple; the history poller keeps its own long-lived context.

use clipboard_rs::{Clipboard, ClipboardContext};

pub fn get_text() -> Option<String> {
    let ctx = ClipboardContext::new().ok()?;
    ctx.get_text().ok()
}

pub fn set_text(text: &str) -> bool {
    let Ok(ctx) = ClipboardContext::new() else {
        return false;
    };
    ctx.set_text(text.to_string()).is_ok()
}

/// Put actual files on the clipboard (CF_HDROP on Windows, file URLs on
/// macOS) — what "screenshot to clipboard" pastes into Explorer/Finder.
pub fn set_files(paths: &[String]) -> bool {
    let Ok(ctx) = ClipboardContext::new() else {
        return false;
    };
    ctx.set_files(paths.to_vec()).is_ok()
}
