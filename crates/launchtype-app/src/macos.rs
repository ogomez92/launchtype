//! macOS-only window activation.
//!
//! The bundle sets `LSUIElement`, so Launchtype is an accessory app with no
//! Dock icon, summoned by the global hotkey from whatever the user is doing.
//! Showing a window is not enough to get the keyboard there, and two separate
//! things have to be true:
//!
//! 1. The *application* must be active. `Show`/`Raise` only order a window
//!    within an app that is already frontmost; an accessory app answering a
//!    hotkey is in the background, so keystrokes keep going to the app the user
//!    came from.
//! 2. The *window* must be key. wxWidgets' `Raise` maps to `orderFront:`, which
//!    puts the window on screen without giving it key status, and a window that
//!    is not key delivers nothing to the focused control.
//!
//! Both also gate speech. Prism's VoiceOver backend announces by posting
//! `NSAccessibilityAnnouncementRequestedNotification` against a window, and
//! VoiceOver acts on those only for the frontmost application — so until the
//! app activates, every announcement is silently dropped.
//!
//! wxWidgets exposes neither call, so both are made here directly.
//!
//! Going away has the mirror problem. Hiding the last window of an accessory
//! app leaves the app active with nothing to type into, so the keyboard and
//! the VoiceOver cursor are stranded until the user clicks or Cmd+Tabs back.
//! [`return_to_previous_app`] hands activation back to whoever had it.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, Message};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSRunningApplication, NSView, NSWindow,
    NSWorkspace,
};

thread_local! {
    /// The app that was frontmost when Launchtype was summoned. Only touched on
    /// the main thread, where AppKit lives.
    static PREVIOUS_APP: RefCell<Option<Retained<NSRunningApplication>>> =
        const { RefCell::new(None) };
}

/// Bring Launchtype to the front and make `native_handle`'s window key.
///
/// `native_handle` is `WxWidget::get_handle()` for the frame. A null handle
/// still activates the app, which is the half of the job that does not need it.
///
/// # Safety
///
/// `native_handle` must be null or a live Objective-C object owned by the
/// frame, so it has to be read on the main thread while the frame is alive.
pub unsafe fn activate_window(native_handle: *mut std::ffi::c_void) {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("activate_window called off the main thread; skipping");
        return;
    };

    // Must be read before activating, which makes us the frontmost app.
    remember_frontmost_app();

    let app = NSApplication::sharedApplication(mtm);
    // Deprecated since macOS 14 in favour of `activate`, but the replacement
    // defers to whichever app is currently active, and an accessory app
    // answering a global hotkey is exactly the case that has to override it.
    // The bundle supports macOS 12, where `activate` alone does nothing.
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    if let Some(window) = unsafe { window_for(native_handle) } {
        window.makeKeyAndOrderFront(None);
    }
}

/// Note the frontmost app so [`return_to_previous_app`] can go back to it.
/// Launchtype itself is skipped: a second hotkey press while the window is up
/// must not overwrite the app the user actually came from.
fn remember_frontmost_app() {
    let Some(front) = NSWorkspace::sharedWorkspace().frontmostApplication() else { return };
    if front == NSRunningApplication::currentApplication() {
        return;
    }
    log::info!("summoned from {:?}", front.localizedName());
    PREVIOUS_APP.with(|previous| *previous.borrow_mut() = Some(front));
}

/// Give activation back to the app that was in front when Launchtype was
/// summoned, so its focused control gets the keyboard again. Call right after
/// hiding the window, and only where nothing else is about to take the
/// foreground — a launched app activates itself, and racing it would hand the
/// keyboard to the wrong one.
///
/// Does nothing when Launchtype is no longer active: the user has already gone
/// somewhere else, and pulling them back would be worse than not helping.
pub fn return_to_previous_app() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    if !app.isActive() {
        return;
    }
    let previous = PREVIOUS_APP.with(|previous| previous.borrow_mut().take());
    match previous {
        Some(previous) if !previous.isTerminated() => {
            let activated = previous.activateWithOptions(NSApplicationActivationOptions::empty());
            log::info!("returning to {:?}: activated={activated}", previous.localizedName());
        }
        // Nobody to return to (or they quit meanwhile): stepping aside still
        // lets the window server pick the next app instead of leaving the
        // keyboard with a windowless one.
        _ => {
            log::info!("no previous app to return to; deactivating");
            app.deactivate();
        }
    }
}

/// Report what AppKit actually believes about activation, at `info` level.
///
/// Call after the focus has been set. None of this is observable from the Rust
/// side of wxWidgets, and "the window looked visible" is exactly what made the
/// original bug hard to see — a silent app and a dead input field both look the
/// same from outside. Run the binary inside the bundle with `RUST_LOG=info` to
/// tell "never became key" apart from "key, but nothing is focused".
///
/// # Safety
///
/// Same as [`activate_window`].
pub unsafe fn log_activation_state(native_handle: *mut std::ffi::c_void) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(window) = (unsafe { window_for(native_handle) }) else { return };

    let focused = window
        .firstResponder()
        .map(|r| r.class().name().to_string_lossy().into_owned())
        .unwrap_or_else(|| "none".to_string());
    log::info!(
        "activation: app_active={}, window_key={}, first_responder={focused}",
        NSApplication::sharedApplication(mtm).isActive(),
        window.isKeyWindow(),
    );
}

/// The `NSWindow` behind a wxWidgets native handle.
///
/// wxOSX hands back the content `NSView` for most windows but the `NSWindow`
/// itself for some top-level ones, and which one it is has changed between
/// wxWidgets versions. Asking the object what it is costs nothing and does not
/// silently do the wrong thing if that detail changes again.
unsafe fn window_for(native_handle: *mut std::ffi::c_void) -> Option<Retained<NSWindow>> {
    if native_handle.is_null() {
        log::warn!("frame has no native handle; activating the app only");
        return None;
    }
    let object: &AnyObject = unsafe { &*native_handle.cast::<AnyObject>() };
    if let Some(window) = object.downcast_ref::<NSWindow>() {
        return Some(window.retain());
    }
    if let Some(view) = object.downcast_ref::<NSView>() {
        return view.window();
    }
    log::warn!("frame handle is neither NSWindow nor NSView; activating the app only");
    None
}
