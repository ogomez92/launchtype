//! Modal dialogs — ports of the `src/ui/*.py` dialogs. Nested modals are
//! avoided structurally: OK handlers only validate and end the modal; any
//! follow-up dialog (the "Edit Assistant" question) opens after `show_modal`
//! returns, which sidesteps the Windows default-button double-click bug the
//! Python code worked around with wx.CallAfter.

use launchtype_core::i18n::tr;
use launchtype_core::model::Command;
use wxdragon::dialogs::dir_dialog::DirDialog;
use wxdragon::dialogs::file_dialog::{FileDialog, FileDialogStyle};
use wxdragon::dialogs::message_dialog::{MessageDialog, MessageDialogStyle};
use wxdragon::dialogs::Dialog;
use wxdragon::prelude::*;

use crate::controller::ModeController;

const ID_OK: i32 = wxdragon::id::ID_OK as i32;
const ID_YES: i32 = wxdragon::id::ID_YES as i32;

/// Model ids offered in the AI-model dropdown, in display order.
const AI_MODEL_IDS: [&str; 3] = ["claude-opus-4-8", "claude-sonnet-5", "claude-haiku-4-5"];

fn error_box(parent: &Dialog, text: &str, title: &str) {
    MessageDialog::builder(parent, text, title)
        .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconError)
        .build()
        .show_modal();
}

fn question_box(parent: &Frame, text: &str, title: &str) -> bool {
    let result = MessageDialog::builder(parent, text, title)
        .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
        .build()
        .show_modal();
    result == ID_YES
}

fn labeled_row(dialog: &Dialog, sizer: &BoxSizer, label: &str) -> TextCtrl {
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    let text_label = StaticText::builder(dialog).with_label(label).build();
    let entry = TextCtrl::builder(dialog).build();
    row.add(&text_label, 0, SizerFlag::All, 0);
    row.add(&entry, 0, SizerFlag::All, 0);
    sizer.add_sizer(&row, 0, SizerFlag::All, 0);
    entry
}

fn ok_cancel_row(dialog: &Dialog, sizer: &BoxSizer) -> (Button, Button) {
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    let ok = Button::builder(dialog).with_label(&tr("&OK")).build();
    let cancel = Button::builder(dialog).with_label(&tr("&Cancel")).build();
    ok.set_default();
    row.add(&ok, 0, SizerFlag::All, 0);
    row.add(&cancel, 0, SizerFlag::All, 0);
    sizer.add_sizer(&row, 0, SizerFlag::All, 0);
    (ok, cancel)
}

/// Show the Add/Edit/Copy command dialog and apply the result to the store.
/// `seed`: `None` = add; a command with an empty name = "add from copy";
/// otherwise edit. Returns true when a command was saved.
pub fn command_edition_dialog(
    parent: &Frame,
    controller: &mut ModeController,
    seed: Option<Command>,
) -> bool {
    let is_copying = seed.as_ref().is_some_and(|c| c.name.is_empty());
    let is_editing = seed.is_some() && !is_copying;
    let title = if is_editing {
        tr("Edit Command")
    } else if is_copying {
        tr("Add command from copy")
    } else {
        tr("Add Command")
    };

    let dialog = Dialog::builder(parent, &title).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let help = StaticText::builder(&dialog)
        .with_label(&tr("Enter the information about the command you wish to add:"))
        .build();
    sizer.add(&help, 0, SizerFlag::All, 0);

    let path_row = BoxSizer::builder(Orientation::Horizontal).build();
    let path_label = StaticText::builder(&dialog).with_label(&tr("&Path to file:")).build();
    let path_entry = TextCtrl::builder(&dialog).build();
    let browse = Button::builder(&dialog).with_label(&tr("&Browse...")).build();
    path_row.add(&path_label, 0, SizerFlag::All, 0);
    path_row.add(&path_entry, 0, SizerFlag::All, 0);
    path_row.add(&browse, 0, SizerFlag::All, 0);
    sizer.add_sizer(&path_row, 0, SizerFlag::All, 0);

    let args_entry = labeled_row(&dialog, &sizer, &tr("&Arguments (optional, comma separated):"));
    let name_entry = labeled_row(&dialog, &sizer, &tr("Display &Name:"));
    let shortcut_entry = labeled_row(&dialog, &sizer, &tr("&Shortcut (optional):"));
    let admin_checkbox = CheckBox::builder(&dialog).with_label(&tr("Run as &administrator")).build();
    sizer.add(&admin_checkbox, 0, SizerFlag::All, 0);
    let (ok, cancel) = ok_cancel_row(&dialog, &sizer);
    dialog.set_sizer(sizer, true);

    if let Some(command) = &seed {
        path_entry.set_value(&command.path);
        args_entry.set_value(command.args());
        name_entry.set_value(&command.name);
        shortcut_entry.set_value(command.shortcut());
        admin_checkbox.set_value(command.run_as_admin());
    }

    {
        let dialog = dialog;
        browse.on_click(move |_| {
            let file_dialog = FileDialog::builder(&dialog)
                .with_message(&tr("Choose a file"))
                .with_default_dir(&std::env::current_dir().unwrap_or_default().to_string_lossy())
                .with_wildcard("*.*")
                .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
                .build();
            if file_dialog.show_modal() == ID_OK {
                if let Some(path) = file_dialog.get_path() {
                    path_entry.set_value(&path);
                }
            }
        });
    }
    {
        // Validation errors stay inside the modal (matching Python); success
        // ends the modal and the save happens after show_modal returns.
        let dialog = dialog;
        let existing_shortcuts: Vec<String> = controller
            .commands
            .file
            .commands
            .iter()
            .map(|c| c.shortcut().to_string())
            .collect();
        ok.on_click(move |_| {
            if !std::path::Path::new(&path_entry.get_value()).exists() {
                error_box(&dialog, &tr("This path is incorrect."), "Error");
                return;
            }
            if name_entry.get_value().is_empty() {
                error_box(
                    &dialog,
                    &tr("The command must have a display name."),
                    &tr("No display name provided"),
                );
                return;
            }
            let shortcut = shortcut_entry.get_value().to_lowercase();
            if !is_editing && !shortcut.is_empty() && existing_shortcuts.iter().any(|s| *s == shortcut) {
                error_box(&dialog, &tr("The shortcut is already in use."), &tr("Shortcut taken"));
                return;
            }
            dialog.end_modal(ID_OK);
        });
    }
    {
        let dialog = dialog;
        cancel.on_click(move |_| dialog.end_modal(wxdragon::id::ID_CANCEL as i32));
    }

    if dialog.show_modal() != ID_OK {
        return false;
    }

    // Editing rebuilds the command from scratch (delete + re-add), carrying
    // its usage count across so stats mode doesn't reset on every edit.
    let mut preserved_run_count = 0;
    if is_editing {
        let old = seed.as_ref().unwrap();
        preserved_run_count = old.run_count();
        controller.commands.pop_by_uuid(&old.id);

        let new_path = path_entry.get_value();
        if old.path != new_path {
            let same_path: Vec<String> = controller
                .commands
                .file
                .commands
                .iter()
                .filter(|c| c.path == old.path)
                .map(|c| c.name.clone())
                .collect();
            if !same_path.is_empty() {
                let mut names = String::new();
                for name in same_path.iter().take(5) {
                    names.push_str(name);
                    names.push_str(", ");
                }
                if same_path.len() > 5 {
                    names.push_str(&tr("and "));
                    names.push_str(&(same_path.len() - 5).to_string());
                    names.push_str(&tr(" more. "));
                }
                let text = format!(
                    "{}{}{}",
                    tr("This path is already in use by the following actions: "),
                    names,
                    tr("Do you want to change the path for all of them?")
                );
                if question_box(parent, &text, &tr("Edit Assistant")) {
                    let old_path = old.path.clone();
                    for command in &mut controller.commands.file.commands {
                        if command.path == old_path {
                            command.path = new_path.clone();
                        }
                    }
                    controller.commands.sync();
                }
            }
        }
    }

    controller.commands.add_command(
        &path_entry.get_value(),
        &name_entry.get_value(),
        &args_entry.get_value(),
        &shortcut_entry.get_value(),
        admin_checkbox.get_value(),
        preserved_run_count,
    );
    true
}

/// Settings dialog; returns true when saved (caller refreshes sound enable).
pub fn settings_dialog(
    parent: &Frame,
    settings: &mut launchtype_core::settings::SettingsStore,
) -> bool {
    let dialog = Dialog::builder(parent, &tr("Settings")).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();

    let sounds_cb = CheckBox::builder(&dialog).with_label(&tr("Enable &sounds")).build();
    sounds_cb.set_value(settings.settings.enable_sounds);
    sizer.add(&sounds_cb, 0, SizerFlag::All, 5);
    let minimized_cb = CheckBox::builder(&dialog).with_label(&tr("Start &minimized")).build();
    minimized_cb.set_value(settings.settings.start_minimized);
    sizer.add(&minimized_cb, 0, SizerFlag::All, 5);
    let snippets_cb = CheckBox::builder(&dialog)
        .with_label(&tr("Start in s&nippets mode when invoked"))
        .build();
    snippets_cb.set_value(settings.settings.snippets_on_invoke);
    sizer.add(&snippets_cb, 0, SizerFlag::All, 5);

    let steam_label = StaticText::builder(&dialog).with_label(&tr("Steam &library path:")).build();
    sizer.add(&steam_label, 0, SizerFlag::All, 5);
    let steam_row = BoxSizer::builder(Orientation::Horizontal).build();
    let steam_entry = TextCtrl::builder(&dialog).build();
    steam_entry.set_value(&settings.settings.steam_library);
    let browse = Button::builder(&dialog).with_label(&tr("&Browse...")).build();
    steam_row.add(&steam_entry, 1, SizerFlag::Expand, 5);
    steam_row.add(&browse, 0, SizerFlag::All, 0);
    sizer.add_sizer(&steam_row, 0, SizerFlag::Expand, 5);

    let ai_label = StaticText::builder(&dialog)
        .with_label(&tr("AI model for screenshot descriptions:"))
        .build();
    sizer.add(&ai_label, 0, SizerFlag::All, 5);
    let ai_choice = Choice::builder(&dialog).build();
    ai_choice.append(&tr("Claude Opus (best quality)"));
    ai_choice.append(&tr("Claude Sonnet (balanced)"));
    ai_choice.append(&tr("Claude Haiku (fastest, lightest)"));
    let selected = AI_MODEL_IDS
        .iter()
        .position(|id| *id == settings.settings.ai_model)
        .unwrap_or(0);
    ai_choice.set_selection(selected as u32);
    sizer.add(&ai_choice, 0, SizerFlag::Expand, 5);

    let hint = StaticText::builder(&dialog)
        .with_label(&tr("Command line flags override these settings for the current run."))
        .build();
    sizer.add(&hint, 0, SizerFlag::All, 5);

    let (ok, cancel) = ok_cancel_row(&dialog, &sizer);
    dialog.set_sizer(sizer, true);

    {
        let dialog = dialog;
        browse.on_click(move |_| {
            let dir_dialog = DirDialog::builder(
                &dialog,
                &tr("Choose Steam library folder"),
                &steam_entry.get_value(),
            )
            .build();
            if dir_dialog.show_modal() == ID_OK {
                if let Some(path) = dir_dialog.get_path() {
                    steam_entry.set_value(&path);
                }
            }
        });
    }
    {
        let dialog = dialog;
        ok.on_click(move |_| dialog.end_modal(ID_OK));
    }
    {
        let dialog = dialog;
        cancel.on_click(move |_| dialog.end_modal(wxdragon::id::ID_CANCEL as i32));
    }

    if dialog.show_modal() != ID_OK {
        return false;
    }

    settings.settings.enable_sounds = sounds_cb.get_value();
    settings.settings.start_minimized = minimized_cb.get_value();
    settings.settings.snippets_on_invoke = snippets_cb.get_value();
    settings.settings.steam_library = steam_entry.get_value();
    let ai_index = ai_choice.get_selection().unwrap_or(0) as usize;
    settings.settings.ai_model = AI_MODEL_IDS[ai_index.min(AI_MODEL_IDS.len() - 1)].to_string();
    let _ = settings.save();
    true
}

fn sound_file_row(dialog: &Dialog, sizer: &BoxSizer) -> TextCtrl {
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    let label = StaticText::builder(dialog).with_label(&tr("&Sound file (optional):")).build();
    let entry = TextCtrl::builder(dialog).build();
    let browse = Button::builder(dialog).with_label(&tr("&Browse...")).build();
    row.add(&label, 0, SizerFlag::All, 0);
    row.add(&entry, 0, SizerFlag::All, 0);
    row.add(&browse, 0, SizerFlag::All, 0);
    sizer.add_sizer(&row, 0, SizerFlag::All, 0);
    {
        let dialog = *dialog;
        browse.on_click(move |_| {
            let file_dialog = FileDialog::builder(&dialog)
                .with_message(&tr("Choose a sound file"))
                .with_wildcard(&tr("Sound files (*.wav)|*.wav|All files (*.*)|*.*"))
                .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
                .build();
            if file_dialog.show_modal() == ID_OK {
                if let Some(path) = file_dialog.get_path() {
                    entry.set_value(&path);
                }
            }
        });
    }
    entry
}

/// Add-timer dialog; adds to the store on OK. Returns true when added.
pub fn add_timer_dialog(parent: &Frame, controller: &mut ModeController) -> bool {
    let dialog = Dialog::builder(parent, &tr("Add Timer")).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let title_entry = labeled_row(&dialog, &sizer, &tr("&Title:"));
    let desc_entry = labeled_row(&dialog, &sizer, &tr("&Description:"));

    let minutes_row = BoxSizer::builder(Orientation::Horizontal).build();
    let minutes_label = StaticText::builder(&dialog).with_label(&tr("&Minutes:")).build();
    let minutes_spin = SpinCtrl::builder(&dialog).with_min_value(1).with_max_value(1440).with_initial_value(5).build();
    minutes_row.add(&minutes_label, 0, SizerFlag::All, 0);
    minutes_row.add(&minutes_spin, 0, SizerFlag::All, 0);
    sizer.add_sizer(&minutes_row, 0, SizerFlag::All, 0);

    let repeating_cb = CheckBox::builder(&dialog)
        .with_label(&tr("&Repeating (fires every X minutes until disabled)"))
        .build();
    sizer.add(&repeating_cb, 0, SizerFlag::All, 0);
    let sound_entry = sound_file_row(&dialog, &sizer);
    let (ok, cancel) = ok_cancel_row(&dialog, &sizer);
    dialog.set_sizer(sizer, true);

    {
        let dialog = dialog;
        ok.on_click(move |_| {
            if title_entry.get_value().is_empty() {
                error_box(&dialog, &tr("Please enter a title for the timer."), "Error");
                return;
            }
            dialog.end_modal(ID_OK);
        });
    }
    {
        let dialog = dialog;
        cancel.on_click(move |_| dialog.end_modal(wxdragon::id::ID_CANCEL as i32));
    }

    if dialog.show_modal() != ID_OK {
        return false;
    }
    let sound = sound_entry.get_value();
    controller.timers.add(
        launchtype_core::timers::TimerDef::new(
            title_entry.get_value(),
            desc_entry.get_value(),
            minutes_spin.value().max(1) as u64,
            repeating_cb.get_value(),
            Some(sound),
        ),
        controller.clock.now(),
    );
    true
}

/// Add-alarm dialog; adds to the store on OK. Returns true when added.
pub fn add_alarm_dialog(parent: &Frame, controller: &mut ModeController) -> bool {
    let dialog = Dialog::builder(parent, &tr("Add Alarm")).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let title_entry = labeled_row(&dialog, &sizer, &tr("&Title:"));
    let desc_entry = labeled_row(&dialog, &sizer, &tr("&Description:"));

    let hour_row = BoxSizer::builder(Orientation::Horizontal).build();
    let hour_label = StaticText::builder(&dialog).with_label(&tr("&Hour (0-23):")).build();
    let hour_spin = SpinCtrl::builder(&dialog).with_min_value(0).with_max_value(23).with_initial_value(8).build();
    hour_row.add(&hour_label, 0, SizerFlag::All, 0);
    hour_row.add(&hour_spin, 0, SizerFlag::All, 0);
    sizer.add_sizer(&hour_row, 0, SizerFlag::All, 0);

    let minute_row = BoxSizer::builder(Orientation::Horizontal).build();
    let minute_label = StaticText::builder(&dialog).with_label(&tr("&Minute (0-59):")).build();
    let minute_spin = SpinCtrl::builder(&dialog).with_min_value(0).with_max_value(59).with_initial_value(0).build();
    minute_row.add(&minute_label, 0, SizerFlag::All, 0);
    minute_row.add(&minute_spin, 0, SizerFlag::All, 0);
    sizer.add_sizer(&minute_row, 0, SizerFlag::All, 0);

    let sound_entry = sound_file_row(&dialog, &sizer);
    let (ok, cancel) = ok_cancel_row(&dialog, &sizer);
    dialog.set_sizer(sizer, true);

    {
        let dialog = dialog;
        ok.on_click(move |_| {
            if title_entry.get_value().is_empty() {
                error_box(&dialog, &tr("Please enter a title for the alarm."), "Error");
                return;
            }
            dialog.end_modal(ID_OK);
        });
    }
    {
        let dialog = dialog;
        cancel.on_click(move |_| dialog.end_modal(wxdragon::id::ID_CANCEL as i32));
    }

    if dialog.show_modal() != ID_OK {
        return false;
    }
    let sound = sound_entry.get_value();
    controller.alarms.add(launchtype_core::alarms::AlarmDef::new(
        title_entry.get_value(),
        desc_entry.get_value(),
        hour_spin.value().clamp(0, 23) as u32,
        minute_spin.value().clamp(0, 59) as u32,
        Some(sound),
    ));
    true
}

/// Add/edit snippet dialog. `existing` = (shortcut, contents) when editing.
/// Returns true when saved (caller reloads snippets).
pub fn snippet_dialog(parent: &Frame, existing: Option<(String, String)>) -> bool {
    let dialog = Dialog::builder(parent, &tr("New snippet")).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let name_entry = labeled_row(&dialog, &sizer, &tr("Name:"));
    let contents_label = StaticText::builder(&dialog).with_label(&tr("Contents:")).build();
    sizer.add(&contents_label, 0, SizerFlag::All, 0);
    let contents_entry = TextCtrl::builder(&dialog)
        .with_style(wxdragon::widgets::textctrl::TextCtrlStyle::MultiLine)
        .build();
    sizer.add(&contents_entry, 1, SizerFlag::Expand, 0);
    let (ok, cancel) = ok_cancel_row(&dialog, &sizer);
    dialog.set_sizer(sizer, true);

    let original_shortcut = existing.as_ref().map(|(shortcut, _)| shortcut.clone());
    if let Some((shortcut, contents)) = &existing {
        name_entry.set_value(shortcut);
        contents_entry.set_value(contents);
    }

    {
        let dialog = dialog;
        ok.on_click(move |_| {
            if name_entry.get_value().is_empty() || contents_entry.get_value().is_empty() {
                error_box(
                    &dialog,
                    &tr("Please enter a name and contents for the snippet."),
                    "Error",
                );
                return;
            }
            dialog.end_modal(ID_OK);
        });
    }
    {
        let dialog = dialog;
        cancel.on_click(move |_| dialog.end_modal(wxdragon::id::ID_CANCEL as i32));
    }

    if dialog.show_modal() != ID_OK {
        return false;
    }
    let name = name_entry.get_value();
    let contents = contents_entry.get_value();
    let result = match original_shortcut {
        Some(original) => launchtype_services::snippets::update_snippet(&original, &name, &contents),
        None => launchtype_services::snippets::write_snippet(&name, &contents),
    };
    if let Err(e) = result {
        log::warn!("snippet save failed: {e}");
    }
    true
}

/// Prompt for Notebrook credentials; returns Some((url, token)) on OK.
pub fn notebrook_credentials_dialog(
    parent: &Frame,
    current_url: &str,
    current_token: &str,
) -> Option<(String, String)> {
    let dialog = Dialog::builder(parent, &tr("Notebrook credentials")).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let help = StaticText::builder(&dialog)
        .with_label(&tr(
            "Enter your Notebrook server URL and access token. They are stored locally in settings.json.",
        ))
        .build();
    sizer.add(&help, 0, SizerFlag::All, 5);
    let url_entry = labeled_row(&dialog, &sizer, &tr("Server &URL:"));
    url_entry.set_value(current_url);
    let token_entry = labeled_row(&dialog, &sizer, &tr("&Token:"));
    token_entry.set_value(current_token);
    let (ok, cancel) = ok_cancel_row(&dialog, &sizer);
    dialog.set_sizer(sizer, true);

    {
        let dialog = dialog;
        ok.on_click(move |_| {
            if url_entry.get_value().trim().is_empty() || token_entry.get_value().trim().is_empty()
            {
                error_box(
                    &dialog,
                    &tr("Please enter both the server URL and the token."),
                    &tr("Error"),
                );
                return;
            }
            dialog.end_modal(ID_OK);
        });
    }
    {
        let dialog = dialog;
        cancel.on_click(move |_| dialog.end_modal(wxdragon::id::ID_CANCEL as i32));
    }

    if dialog.show_modal() != ID_OK {
        return None;
    }
    Some((
        url_entry.get_value().trim().to_string(),
        token_entry.get_value().trim().to_string(),
    ))
}
