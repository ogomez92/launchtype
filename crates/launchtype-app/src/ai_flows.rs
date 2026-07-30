//! Screenshot AI flows (describe / explore regions / grab specific region) —
//! port of the AI half of `ui_manager.py`. Workers run on plain threads and
//! marshal results back with `wxdragon::call_after`; the shell is reached
//! from those closures via the main-thread registry in [`crate::shell`],
//! because `Rc` handles cannot cross threads.

use launchtype_core::i18n::{format_args, tr, Arg};
use wxdragon::prelude::WxWidget;
use launchtype_core::mode::UiMode;
use launchtype_services::screenshot::{self, RgbaImage};
use launchtype_services::sounds::SoundPlayer;

use crate::shell::{update_list, with_shell, SharedShell};
use crate::speech::speak_now;

/// Localized prompt asking the AI to describe a screenshot for a blind user.
/// Its translation also sets the language the AI answers in.
fn describe_prompt() -> String {
    tr("You are describing a screenshot for a blind person who cannot see the screen. Describe clearly and naturally what is shown: the application or window, the main text and controls, the layout, and anything important. Be thorough but concise, and write it to be read aloud. Answer in English.")
}

fn regions_prompt() -> String {
    tr("You are helping a blind person explore a screenshot. Identify the most useful distinct regions of this image, such as dialogs, toolbars, text areas, images, button groups or notifications. Reply with ONLY a JSON array and no other text, where each item is {\"label\": a short description, \"box\": [x1, y1, x2, y2] in pixels of this image}. Use at most 8 regions, ordered by importance. Write the labels in English.")
}

fn locate_prompt(query: &str) -> String {
    let request = format_args(
        &tr("Find this element in the image: {query}"),
        &[("query", Arg::Str(query))],
    );
    let instructions = tr("You are helping a blind person crop a screenshot. Reply with ONLY a JSON object and no other text. If you find the requested element, reply {\"found\": true, \"box\": [x1, y1, x2, y2] in pixels of this image}. If you cannot find it, reply {\"found\": false, \"reason\": a short explanation of why, usually that the element is not visible}. Write the reason in English.");
    format!("{request}\n\n{instructions}")
}

/// The single exit for every screenshot failure: logged, spoken, *and* shown
/// in a dialog. Runs on the UI thread — background workers marshal back with
/// `wxdragon::call_after` first.
///
/// Speaking alone was not enough. These flows hide the window before they
/// capture and then work for seconds, so a failure that is only spoken is
/// invisible whenever the speech backend is not running: the action simply
/// looked like it did nothing at all. The dialog is what makes that
/// impossible, and it brings the window back up on its way in.
fn report_failure(message: &str) {
    log::warn!("screenshot action failed: {message}");
    with_shell(|shell| {
        // No error.wav ships in sounds/ yet, so this is a no-op until one is
        // dropped in — the same arrangement as the "type" keystroke sound.
        shell.borrow().sounds.play("error");
        speak_now(message, true);
    });
    // The dialog itself waits for the next turn of the event loop. Some of
    // these failures are reported straight out of the Run handler, and Run is
    // the frame's default button: a modal opened from a default button's own
    // handler eats the very keypress that opened it, so the user has to press
    // Enter twice. Deferring is the same fix the Python code used.
    let message = message.to_string();
    wxdragon::call_after(Box::new(move || {
        with_shell(|shell| crate::shell::report_error(shell, &tr("Screenshots"), &message));
    }));
}

fn report_capture_failed(reason: &str) {
    report_failure(&format_args(
        &tr("The screenshot could not be taken: {reason}"),
        &[("reason", Arg::Str(reason))],
    ));
}

fn report_ai_error(reason: &str) {
    report_failure(&format_args(
        &tr("Could not analyze the screenshot: {reason}"),
        &[("reason", Arg::Str(reason))],
    ));
}

fn report_crop_failed(reason: &str) {
    report_failure(&format_args(
        &tr("The screenshot could not be cropped because {reason}"),
        &[("reason", Arg::Str(reason))],
    ));
}

/// Save the image under `screenshots/` and put the FILE on the clipboard,
/// playing the "copy" cue. Returns whether it got there.
///
/// The failure is reported rather than swallowed: every caller goes on to
/// announce that something was copied, and a full disk or a read-only folder
/// used to make that announcement a lie.
fn save_and_copy(image: &RgbaImage, prefix: &str, sounds: &SoundPlayer) -> bool {
    match screenshot::save_and_copy(image, prefix) {
        Ok(_) => {
            sounds.play("copy");
            true
        }
        Err(e) => {
            report_failure(&format_args(
                &tr("The screenshot could not be saved: {reason}"),
                &[("reason", Arg::Str(&e.to_string()))],
            ));
            false
        }
    }
}

fn announce_description(sounds: &SoundPlayer, description: &str) {
    // The image (full capture or crop) is what stays on the clipboard; the
    // description is only spoken.
    sounds.play("match");
    speak_now(description, true);
}

/// Run one screenshot menu item: ask for whatever the action needs, hide the
/// window so the capture never includes Launchtype itself, then start it.
///
/// Hiding happens here rather than in the caller because "grab specific
/// region" has a question to ask first, and a modal parented to a hidden frame
/// does not come to the foreground (see `shell::with_window_up`).
pub fn handle_screenshot_action(shell: &SharedShell, action: &str) {
    let capture_window = action.ends_with("window");

    if action.starts_with("grab") {
        // Asked on the next turn of the event loop, not from this handler:
        // Run is the frame's default button, so the Enter that got here would
        // otherwise be delivered to the dialog as well and dismiss it before
        // it could be typed into (see [`report_failure`]).
        wxdragon::call_after(Box::new(move || {
            with_shell(|shell| {
                let frame = shell.borrow().frame;
                // Cancel means cancel: no capture, and the launcher is left
                // exactly as it was, still on the screenshots list.
                let Some(query) = crate::dialogs::grab_region_dialog(&frame) else {
                    return;
                };
                hide_window(shell);
                grab_specific_region(shell, capture_window, query);
            });
        }));
        return;
    }

    hide_window(shell);
    match action {
        "window" | "screen" => {
            let sounds = shell.borrow().sounds.clone();
            match screenshot::take_screenshot(capture_window) {
                Ok(_) => sounds.play("copy"),
                Err(e) => report_capture_failed(&e.to_string()),
            }
        }
        a if a.starts_with("describe") => describe_screenshot(shell, capture_window),
        a if a.starts_with("regions") => explore_regions(shell, capture_window),
        _ => {}
    }
}

/// Get the launcher out of the shot. Captures wait 300ms (see
/// `screenshot::capture_image`), which is what gives the window time to go.
fn hide_window(shell: &SharedShell) {
    shell.borrow().frame.show(false);
}

fn ai_model(shell: &SharedShell) -> String {
    shell.borrow().settings.settings.ai_model.clone()
}

fn describe_screenshot(shell: &SharedShell, capture_window: bool) {
    let sounds = shell.borrow().sounds.clone();
    sounds.play("run");
    speak_now(&tr("Analyzing screenshot, please wait"), true);
    let model = ai_model(shell);
    let prompt = describe_prompt();

    std::thread::spawn(move || {
        let image = match screenshot::capture_image(capture_window) {
            Ok(image) => image,
            Err(e) => {
                let reason = e.to_string();
                wxdragon::call_after(Box::new(move || report_capture_failed(&reason)));
                return;
            }
        };
        let (image_bytes, _size) = match screenshot::encode_for_ai(&image) {
            Ok(encoded) => encoded,
            Err(e) => {
                let reason = e.to_string();
                wxdragon::call_after(Box::new(move || report_ai_error(&reason)));
                return;
            }
        };
        // Put the actual screenshot on the clipboard (on the UI thread), then
        // describe it; only the text is spoken. A failed save is reported but
        // does not cancel the description — that description is the whole
        // point of this action, the clipboard copy comes along for the ride.
        let copy_sounds = sounds.clone();
        wxdragon::call_after(Box::new(move || {
            save_and_copy(&image, "screenshot", &copy_sounds);
        }));
        match launchtype_services::ai::describe_image(&image_bytes, &prompt, &model) {
            Ok(description) => wxdragon::call_after(Box::new(move || {
                announce_description(&sounds, &description);
            })),
            Err(e) => {
                let reason = e.0;
                wxdragon::call_after(Box::new(move || report_ai_error(&reason)));
            }
        }
    });
}

fn explore_regions(shell: &SharedShell, capture_window: bool) {
    let sounds = shell.borrow().sounds.clone();
    sounds.play("run");
    speak_now(&tr("Finding regions, please wait"), true);
    let model = ai_model(shell);
    let prompt = regions_prompt();

    std::thread::spawn(move || {
        let step = (|| {
            let image = screenshot::capture_image(capture_window).map_err(|e| e.to_string())?;
            let (bytes, sent_size) = screenshot::encode_for_ai(&image).map_err(|e| e.to_string())?;
            let regions = launchtype_services::ai::find_regions(&bytes, &prompt, &model)
                .map_err(|e| e.0)?;
            Ok::<_, String>((image, sent_size, regions))
        })();
        match step {
            Ok((image, sent_size, regions)) => {
                wxdragon::call_after(Box::new(move || {
                    with_shell(|shell| show_regions(shell, image.clone(), sent_size, &regions));
                }));
            }
            Err(reason) => wxdragon::call_after(Box::new(move || report_ai_error(&reason))),
        }
    });
}

fn show_regions(
    shell: &SharedShell,
    image: RgbaImage,
    sent_size: (u32, u32),
    regions: &[launchtype_services::ai::Region],
) {
    let count = regions.len();
    {
        let mut s = shell.borrow_mut();
        s.screenshot_image = Some(image);
        s.screenshot_sent_size = Some(sent_size);
        s.controller.regions =
            regions.iter().map(|r| (r.label.clone(), r.r#box)).collect();
        s.mode = UiMode::Regions;
        s.sounds.play("match");
        if !s.frame.is_shown() {
            s.frame.show(true);
            s.frame.raise();
        }
        s.edit.change_value("");
        s.edit.set_focus();
    }
    update_list(shell);
    speak_now(
        &format_args(
            &tr("{count} regions found. Choose one to crop and describe it."),
            &[("count", Arg::Int(count as i64))],
        ),
        true,
    );
}

/// Region item chosen in the list: crop it out of the last screenshot, copy
/// the crop, and describe it. Keeps the window open.
pub fn crop_and_describe_region(shell: &SharedShell, r#box: [f64; 4]) {
    let (image, sent_size, sounds) = {
        let s = shell.borrow();
        (s.screenshot_image.clone(), s.screenshot_sent_size, s.sounds.clone())
    };
    let (Some(image), Some(sent_size)) = (image, sent_size) else {
        report_failure(&tr("No screenshot is available"));
        return;
    };
    let Some(crop) = screenshot::crop_region(&image, r#box, sent_size) else {
        report_failure(&tr("That region could not be cropped"));
        return;
    };
    // Announce the copy only when it happened; the description follows either
    // way, because the crop is already in hand.
    if save_and_copy(&crop, "region", &sounds) {
        speak_now(&tr("Region copied. Describing it, please wait"), true);
    }
    describe_crop_async(shell, crop);
}

/// Describe a cropped image in the background and speak the result. The crop
/// is already on the clipboard; only the description is spoken.
fn describe_crop_async(shell: &SharedShell, crop: RgbaImage) {
    let sounds = shell.borrow().sounds.clone();
    let model = ai_model(shell);
    let prompt = describe_prompt();
    std::thread::spawn(move || {
        let bytes = match screenshot::encode_for_ai(&crop) {
            Ok((bytes, _)) => bytes,
            Err(e) => {
                let reason = e.to_string();
                wxdragon::call_after(Box::new(move || report_ai_error(&reason)));
                return;
            }
        };
        match launchtype_services::ai::describe_image(&bytes, &prompt, &model) {
            Ok(description) => wxdragon::call_after(Box::new(move || {
                announce_description(&sounds, &description);
            })),
            Err(e) => {
                let reason = e.0;
                wxdragon::call_after(Box::new(move || report_ai_error(&reason)));
            }
        }
    });
}

/// Crop the element the user described out of a fresh screenshot. `query` is
/// what they typed in the "grab a specific region" dialog.
fn grab_specific_region(shell: &SharedShell, capture_window: bool, query: String) {
    let sounds = shell.borrow().sounds.clone();
    sounds.play("run");
    speak_now(
        &format_args(&tr("Looking for {query}, please wait"), &[("query", Arg::Str(&query))]),
        true,
    );
    let model = ai_model(shell);
    let prompt = locate_prompt(&query);

    std::thread::spawn(move || {
        let step = (|| {
            let image = screenshot::capture_image(capture_window).map_err(|e| e.to_string())?;
            let (bytes, sent_size) = screenshot::encode_for_ai(&image).map_err(|e| e.to_string())?;
            let r#box = launchtype_services::ai::locate_region(&bytes, &prompt, &model)
                .map_err(|e| e.0)?;
            Ok::<_, String>((image, sent_size, r#box))
        })();
        match step {
            Ok((image, sent_size, r#box)) => {
                let Some(crop) = screenshot::crop_region(&image, r#box, sent_size) else {
                    wxdragon::call_after(Box::new(move || {
                        report_crop_failed(&tr("the returned area was empty"));
                    }));
                    return;
                };
                wxdragon::call_after(Box::new(move || {
                    with_shell(|shell| {
                        let sounds = shell.borrow().sounds.clone();
                        if save_and_copy(&crop, "crop", &sounds) {
                            speak_now(
                                &format_args(
                                    &tr("Cropped {query} and copied it. Describing it, please wait"),
                                    &[("query", Arg::Str(&query))],
                                ),
                                true,
                            );
                        }
                        describe_crop_async(shell, crop.clone());
                    });
                }));
            }
            Err(reason) => wxdragon::call_after(Box::new(move || report_crop_failed(&reason))),
        }
    });
}
