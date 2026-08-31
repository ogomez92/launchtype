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

/// Empty the clipboard — how a copied vault secret is taken back off it once
/// it has had long enough to be pasted.
pub fn clear() -> bool {
    let Ok(ctx) = ClipboardContext::new() else {
        return false;
    };
    ctx.clear().is_ok()
}

/// Put actual files on the clipboard (CF_HDROP on Windows, file URLs on
/// macOS) — what "screenshot to clipboard" pastes into Explorer/Finder.
pub fn set_files(paths: &[String]) -> bool {
    let Ok(ctx) = ClipboardContext::new() else {
        return false;
    };
    ctx.set_files(paths.to_vec()).is_ok()
}

/// The files on the clipboard, as paths. Empty when the clipboard holds
/// something else.
pub fn get_files() -> Vec<String> {
    let Ok(ctx) = ClipboardContext::new() else {
        return Vec::new();
    };
    ctx.get_files().unwrap_or_default()
}

/// What path mode (`/`) is looking at: the files on the clipboard, or the
/// paths written in the text on it.
///
/// File objects win when there are any — they are unambiguous, and they are
/// what copying in Explorer or Finder produces. Text is the fallback, for
/// "Copy as path", a path pasted out of a terminal, or a `file://` URL from a
/// browser.
///
/// Only candidates that already look like absolute locations are ever probed
/// against the disk (see [`launchtype_core::paths::parse_paths`]), so a
/// clipboard full of prose costs nothing and a mistyped network path cannot
/// turn this into a wait.
pub fn targets() -> Vec<launchtype_core::paths::Target> {
    let mut candidates = get_files();
    if candidates.is_empty() {
        candidates = launchtype_core::paths::parse_paths(&get_text().unwrap_or_default());
    }
    candidates
        .into_iter()
        .filter_map(|candidate| {
            // One `metadata` call answers both questions at once: whether it is
            // there at all, and whether it is a folder.
            let metadata = std::fs::metadata(&candidate).ok()?;
            Some(launchtype_core::paths::Target::new(candidate, metadata.is_dir()))
        })
        .collect()
}
