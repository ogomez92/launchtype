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

fn speak_ai_error(reason: &str) {
    speak_now(
        &format_args(&tr("Could not analyze the screenshot: {reason}"), &[("reason", Arg::Str(reason))]),
        true,
    );
}

fn speak_crop_failed(reason: &str) {
    speak_now(
        &format_args(
            &tr("The screenshot could not be cropped because {reason}"),
            &[("reason", Arg::Str(reason))],
        ),
        true,
    );
}

fn announce_description(sounds: &SoundPlayer, description: &str) {
    // The image (full capture or crop) is what stays on the clipboard; the
    // description is only spoken.
    sounds.play("match");
    speak_now(description, true);
}

/// Run one screenshot menu item. The window is already hidden by the caller,
/// so captures never include Launchtype itself.
pub fn handle_screenshot_action(shell: &SharedShell, action: &str) -> Result<(), String> {
    let capture_window = action.ends_with("window");
    match action {
        "window" | "screen" => {
            let sounds = shell.borrow().sounds.clone();
            screenshot::take_screenshot(capture_window).map_err(|e| e.to_string())?;
            sounds.play("copy");
            Ok(())
        }
        a if a.starts_with("describe") => {
            describe_screenshot(shell, capture_window);
            Ok(())
        }
        a if a.starts_with("regions") => {
            explore_regions(shell, capture_window);
            Ok(())
        }
        a if a.starts_with("grab") => {
            grab_specific_region(shell, capture_window);
            Ok(())
        }
        _ => Ok(()),
    }
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
                wxdragon::call_after(Box::new(move || speak_ai_error(&reason)));
                return;
            }
        };
        let (image_bytes, _size) = match screenshot::encode_for_ai(&image) {
            Ok(encoded) => encoded,
            Err(e) => {
                let reason = e.to_string();
                wxdragon::call_after(Box::new(move || speak_ai_error(&reason)));
                return;
            }
        };
        // Put the actual screenshot on the clipboard (on the UI thread), then
        // describe it; only the text is spoken.
        let copy_sounds = sounds.clone();
        wxdragon::call_after(Box::new(move || {
            let _ = screenshot::save_and_copy(&image, "screenshot");
            copy_sounds.play("copy");
        }));
        match launchtype_services::ai::describe_image(&image_bytes, &prompt, &model) {
            Ok(description) => wxdragon::call_after(Box::new(move || {
                announce_description(&sounds, &description);
            })),
            Err(e) => {
                let reason = e.0;
                wxdragon::call_after(Box::new(move || speak_ai_error(&reason)));
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
            Err(reason) => wxdragon::call_after(Box::new(move || speak_ai_error(&reason))),
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
        speak_now(&tr("No screenshot is available"), true);
        return;
    };
    let Some(crop) = screenshot::crop_region(&image, r#box, sent_size) else {
        speak_now(&tr("That region could not be cropped"), true);
        return;
    };
    let _ = screenshot::save_and_copy(&crop, "region");
    sounds.play("copy");
    speak_now(&tr("Region copied. Describing it, please wait"), true);
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
                wxdragon::call_after(Box::new(move || speak_ai_error(&reason)));
                return;
            }
        };
        match launchtype_services::ai::describe_image(&bytes, &prompt, &model) {
            Ok(description) => wxdragon::call_after(Box::new(move || {
                announce_description(&sounds, &description);
            })),
            Err(e) => {
                let reason = e.0;
                wxdragon::call_after(Box::new(move || speak_ai_error(&reason)));
            }
        }
    });
}

/// Crop the element named in the input field out of a fresh screenshot.
fn grab_specific_region(shell: &SharedShell, capture_window: bool) {
    let (query, sounds) = {
        let s = shell.borrow();
        (s.edit.get_value().trim().to_string(), s.sounds.clone())
    };
    if query.is_empty() {
        speak_now(&tr("Type what to grab in the input field first"), true);
        return;
    }
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
                        speak_crop_failed(&tr("the returned area was empty"));
                    }));
                    return;
                };
                wxdragon::call_after(Box::new(move || {
                    let _ = screenshot::save_and_copy(&crop, "crop");
                    with_shell(|shell| {
                        shell.borrow().sounds.play("copy");
                        speak_now(
                            &format_args(
                                &tr("Cropped {query} and copied it. Describing it, please wait"),
                                &[("query", Arg::Str(&query))],
                            ),
                            true,
                        );
                        describe_crop_async(shell, crop.clone());
                    });
                }));
            }
            Err(reason) => wxdragon::call_after(Box::new(move || speak_crop_failed(&reason))),
        }
    });
}
