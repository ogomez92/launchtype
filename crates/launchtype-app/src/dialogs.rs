//! Modal dialogs — ports of the `src/ui/*.py` dialogs. Nested modals are
//! avoided structurally: OK handlers only validate and end the modal; any
//! follow-up dialog (the "Edit Assistant" question) opens after `show_modal`
//! returns, which sidesteps the Windows default-button double-click bug the
//! Python code worked around with wx.CallAfter.

use std::sync::Arc;

use launchtype_core::alarms::AlarmDef;
use launchtype_core::i18n::{format_args, tr, Arg};
use launchtype_core::merge;
use launchtype_core::model::Command;
use launchtype_core::portable::{self, Target, VarSpec};
use launchtype_core::query;
use launchtype_core::timers::TimerDef;
use launchtype_services::sounds::{SoundPlayer, ALARM_SOUNDS, TIMER_SOUNDS};
use wxdragon::dialogs::dir_dialog::DirDialog;
use wxdragon::dialogs::file_dialog::{FileDialog, FileDialogStyle};
use wxdragon::dialogs::message_dialog::{MessageDialog, MessageDialogStyle};
use wxdragon::dialogs::Dialog;
use wxdragon::prelude::*;

use crate::controller::ModeController;

const ID_OK: i32 = wxdragon::id::ID_OK as i32;
const ID_YES: i32 = wxdragon::id::ID_YES as i32;

/// First menu id of each insert-variable menu; a placeholder takes
/// `base + its index in `portable::all_specs()``.
///
/// Every menu on a dialog posts `wxEVT_MENU` to that same dialog, so the two
/// fields need ranges far enough apart that a pick can only ever land in the
/// field it was opened from. The catalog is ~24 entries; 100 apart is ample.
const PATH_VARIABLE_MENU_BASE_ID: i32 = 6300;
const ARGS_VARIABLE_MENU_BASE_ID: i32 = 6400;

/// Model ids offered in the AI-model dropdown, in display order.
const AI_MODEL_IDS: [&str; 3] = ["claude-opus-4-8", "claude-sonnet-5", "claude-haiku-4-5"];

/// Language codes offered in the language dropdown, in display order.
/// `"system"` follows the OS locale; the rest name a shipped catalog.
const LANGUAGE_CODES: [&str; 3] = [launchtype_core::settings::LANGUAGE_SYSTEM, "en", "es"];

/// Exposes `label` as the accessible name of a control that would otherwise
/// announce only its type.
///
/// wxWidgets is compiled with `wxUSE_ACCESSIBILITY`, so its generic
/// `wxWindowAccessible::GetName` shadows native controls. Only `wxButton` reads
/// its `GetLabel()`; every other control falls through to `wxWindow::GetName()`,
/// the internal control name — `"check"`, `"text"`, `"choice"`, `"spinctrl"` —
/// and that non-empty bogus name also suppresses the screen reader's fallback to
/// the adjacent `StaticText`. Setting the window name to the (mnemonic-stripped)
/// label is what makes assistive tech announce the control. Buttons don't need
/// this; static-text labels stay as-is.
fn ax_name<W: WxWidget>(ctrl: &W, label: &str) {
    ctrl.set_name(&strip_mnemonics(label));
}

/// Builds a checkbox with its accessible name wired up. See [`ax_name`].
fn checkbox(parent: &dyn WxWidget, label: &str) -> CheckBox {
    let cb = CheckBox::builder(parent).with_label(label).build();
    ax_name(&cb, label);
    cb
}

/// Strips wx mnemonic markers so the accessible name reads naturally: a lone `&`
/// marks the shortcut key and is dropped, while `&&` is a literal ampersand.
fn strip_mnemonics(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            if chars.peek() == Some(&'&') {
                out.push('&');
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

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
    ax_name(&entry, label);
    row.add(&text_label, 0, SizerFlag::All, 0);
    row.add(&entry, 0, SizerFlag::All, 0);
    sizer.add_sizer(&row, 0, SizerFlag::All, 0);
    entry
}

/// Like [`labeled_row`], but the typed text is masked.
fn labeled_password_row(dialog: &Dialog, sizer: &BoxSizer, label: &str) -> TextCtrl {
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    let text_label = StaticText::builder(dialog).with_label(label).build();
    let entry = TextCtrl::builder(dialog)
        .with_style(wxdragon::widgets::textctrl::TextCtrlStyle::Password)
        .build();
    ax_name(&entry, label);
    row.add(&text_label, 0, SizerFlag::All, 0);
    row.add(&entry, 0, SizerFlag::All, 0);
    sizer.add_sizer(&row, 0, SizerFlag::All, 0);
    entry
}

/// Insert `text` where the caret is and leave focus in the field, so typing
/// carries straight on after picking a variable.
fn insert_at_caret(entry: &TextCtrl, text: &str) {
    let value = entry.get_value();
    // wx counts characters, not bytes; a path may hold non-ASCII.
    let chars: Vec<char> = value.chars().collect();
    let at = (entry.get_insertion_point().max(0) as usize).min(chars.len());
    let mut updated: String = chars[..at].iter().collect();
    updated.push_str(text);
    updated.extend(chars[at..].iter());
    entry.set_value(&updated);
    entry.set_insertion_point((at + text.chars().count()) as i64);
    entry.set_focus();
}

/// The menu line for one placeholder: what to type, what it means, and what it
/// resolves to here — the resolved value is the part that tells the user
/// whether the variable is the one they want.
fn variable_menu_label(spec: &VarSpec) -> String {
    let name = portable::placeholder(spec.name);
    let description = portable::description(spec.name);
    match launchtype_services::portable::vars().display_value(spec.name) {
        Some(value) => format!("{name} - {description} ({value})"),
        // A default-handler variable has no path to show.
        None => format!("{name} - {description}"),
    }
}

/// Add an "insert variable" button to `row` that drops a placeholder into
/// `entry` at the caret.
///
/// The popup mirrors the Alt+M modes menu, which is the accessible pattern
/// already established in this app. `Dialog` does not implement wxdragon's
/// `MenuEvents` trait (and the orphan rule rules out adding it), but it does
/// implement `WxEvtHandler`, so the `wxEVT_MENU` the popup posts is bound
/// directly here.
fn variable_menu_button(
    dialog: &Dialog,
    row: &BoxSizer,
    entry: &TextCtrl,
    label: &str,
    base_id: i32,
) {
    let button = Button::builder(dialog).with_label(label).build();
    row.add(&button, 0, SizerFlag::All, 0);

    // Only offer placeholders this machine can resolve: inserting one with no
    // value (`{{onedrive}}` without OneDrive installed) would leave the literal
    // text in the command. Menu ids are `base_id + index into this list`, so both
    // closures below share the same filtered vec.
    //
    // `{{query}}` leads the menu and is exempt from that filter: it is the one
    // placeholder that is *meant* to have no value here, and it is the reason
    // most people open this menu now.
    let vars = launchtype_services::portable::vars();
    let specs: Vec<VarSpec> = std::iter::once(portable::QUERY_VAR)
        .chain(portable::all_specs().into_iter().filter(|spec| vars.get(spec.name).is_some()))
        .collect();
    {
        let dialog = *dialog;
        let specs = specs.clone();
        button.on_click(move |_| {
            let mut builder = Menu::builder();
            for (index, spec) in specs.iter().enumerate() {
                builder =
                    builder.append_item(base_id + index as i32, &variable_menu_label(spec), "");
            }
            let mut menu = builder.build();
            dialog.popup_menu(&mut menu, None);
        });
    }
    {
        let entry = *entry;
        let count = specs.len() as i32;
        dialog.bind_internal(wxdragon::event::EventType::MENU, move |event| {
            let Some(index) = event.get_id().checked_sub(base_id) else { return };
            // Ignore the other field's menu, which posts to this same dialog.
            if index >= count {
                return;
            }
            if let Some(spec) = specs.get(index as usize) {
                insert_at_caret(&entry, &portable::placeholder(spec.name));
            }
        });
    }
}

/// Rewrite a browsed-for path into placeholder form when one applies, so
/// picking a file out of your user folder does not hardcode it.
fn portabilize(path: &str) -> String {
    let vars = launchtype_services::portable::vars();
    match portable::suggest(path, vars) {
        Some((from, to)) => portable::apply_to_setting(
            path,
            &[portable::Fix { from, to, count: 1 }],
            vars,
        )
        .unwrap_or_else(|| path.to_string()),
        None => path.to_string(),
    }
}

fn ok_cancel_row(dialog: &Dialog, sizer: &BoxSizer) -> (Button, Button) {
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    // Standard ids let wxDialog map Enter -> OK and Escape -> Cancel. Without
    // ID_CANCEL on a button, pressing Escape does nothing.
    let ok = Button::builder(dialog).with_id(ID_OK).with_label(&tr("&OK")).build();
    let cancel = Button::builder(dialog)
        .with_id(wxdragon::id::ID_CANCEL)
        .with_label(&tr("&Cancel"))
        .build();
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
    ax_name(&path_entry, &tr("&Path to file:"));
    let browse = Button::builder(&dialog).with_label(&tr("&Browse...")).build();
    path_row.add(&path_label, 0, SizerFlag::All, 0);
    path_row.add(&path_entry, 0, SizerFlag::All, 0);
    path_row.add(&browse, 0, SizerFlag::All, 0);
    variable_menu_button(
        &dialog,
        &path_row,
        &path_entry,
        // Distinct labels, not two buttons both reading "Variable": a screen
        // reader announces the label, and "which one am I on" must be obvious.
        &tr("Path &variable..."),
        PATH_VARIABLE_MENU_BASE_ID,
    );
    sizer.add_sizer(&path_row, 0, SizerFlag::All, 0);

    let args_row = BoxSizer::builder(Orientation::Horizontal).build();
    let args_title = tr("&Arguments (optional, comma separated):");
    let args_label = StaticText::builder(&dialog).with_label(&args_title).build();
    let args_entry = TextCtrl::builder(&dialog).build();
    ax_name(&args_entry, &args_title);
    args_row.add(&args_label, 0, SizerFlag::All, 0);
    args_row.add(&args_entry, 0, SizerFlag::All, 0);
    variable_menu_button(
        &dialog,
        &args_row,
        &args_entry,
        &tr("Argument var&iable..."),
        ARGS_VARIABLE_MENU_BASE_ID,
    );
    sizer.add_sizer(&args_row, 0, SizerFlag::All, 0);

    let name_entry = labeled_row(&dialog, &sizer, &tr("Display &Name:"));
    let shortcut_entry = labeled_row(&dialog, &sizer, &tr("&Shortcut (optional):"));
    let admin_checkbox = checkbox(&dialog, &tr("Run as &administrator"));
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
                    // Store the portable form: browsing to something in your
                    // user folder must not hardcode that folder.
                    path_entry.set_value(&portabilize(&path));
                }
            }
        });
    }
    {
        // Validation errors stay inside the modal (matching Python); success
        // ends the modal and the save happens after show_modal returns.
        let dialog = dialog;
        // A shortcut may belong to at most one command. When editing, the
        // command's own shortcut must not clash with itself; adds and copies
        // exclude nothing (the original still owns its shortcut).
        let editing_id = if is_editing {
            seed.as_ref().map(|c| c.id.clone())
        } else {
            None
        };
        let existing_shortcuts: Vec<String> = controller
            .commands
            .file
            .commands
            .iter()
            .filter(|c| editing_id.as_deref() != Some(c.id.as_str()))
            .map(|c| c.shortcut().to_string())
            .collect();
        ok.on_click(move |_| {
            let path = path_entry.get_value();
            let vars = launchtype_services::portable::vars();
            // A typo inside {{...}} would otherwise be launched literally.
            let unknown = portable::unknown_placeholders(&path, vars);
            if let Some(name) = unknown.first() {
                error_box(
                    &dialog,
                    &format_args(
                        &tr("There is no variable called {name}. Use the Variable button to pick one."),
                        &[("name", Arg::Str(&portable::placeholder(name)))],
                    ),
                    &tr("Unknown variable"),
                );
                return;
            }
            // {{browser}} names a handler rather than a file, so there is
            // nothing on disk to check — and neither is there when part of the
            // path is only typed in as the command launches.
            if query::count(&path, "") == 0 {
                if let Target::Path(resolved) = portable::resolve_target(&path, vars) {
                    if !std::path::Path::new(&resolved).exists() {
                        error_box(&dialog, &tr("This path is incorrect."), "Error");
                        return;
                    }
                }
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
            if !shortcut.is_empty() && existing_shortcuts.iter().any(|s| *s == shortcut) {
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

/// What the user chose in the portability dialog.
pub enum PortabilityChoice {
    /// Apply these fixes (a subset of the ones offered).
    Fix(Vec<portable::Fix>),
    /// Leave everything alone this time, but ask again next start.
    NotNow,
    /// Leave everything alone and stop checking.
    NeverAsk,
}

/// Offer to replace machine-specific paths with placeholders.
///
/// Fixes are listed one row per *rule*, not per command: the real file has 389
/// commands but only about a dozen distinct rules, and a 389-row list would be
/// unusable with a screen reader.
pub fn portability_dialog(
    parent: &Frame,
    report: &portable::Report,
) -> PortabilityChoice {
    let dialog = Dialog::builder(parent, &tr("Machine-specific paths")).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();

    let help = StaticText::builder(&dialog)
        .with_label(&tr(
            "Some commands point at folders and programs that only exist on this machine, so they will break if you move Launchtype elsewhere. Tick the ones to replace with a variable that each machine resolves for itself.",
        ))
        .build();
    sizer.add(&help, 0, SizerFlag::All, 5);

    let list_title = tr("&Replacements to make:");
    let list_label = StaticText::builder(&dialog).with_label(&list_title).build();
    sizer.add(&list_label, 0, SizerFlag::All, 5);
    let list = CheckListBox::builder(&dialog).build();
    ax_name(&list, &list_title);
    for fix in &report.fixes {
        list.append(&format_args(
            &tr("{from} becomes {to} ({count} used)"),
            &[
                ("from", Arg::Str(&fix.from)),
                ("to", Arg::Str(&fix.to)),
                ("count", Arg::Int(fix.count as i64)),
            ],
        ));
    }
    // Everything on offer is safe and reversible from the Edit dialog, so
    // start with all of it ticked: the common answer is "yes, all of them".
    for index in 0..list.get_count() {
        list.check(index, true);
    }
    sizer.add(&list, 1, SizerFlag::Expand, 5);

    let button_row = BoxSizer::builder(Orientation::Horizontal).build();
    let fix = Button::builder(&dialog).with_id(ID_OK).with_label(&tr("&Fix selected")).build();
    let not_now = Button::builder(&dialog)
        .with_id(wxdragon::id::ID_CANCEL)
        .with_label(&tr("&Not now"))
        .build();
    let never = Button::builder(&dialog).with_label(&tr("Ne&ver ask again")).build();
    fix.set_default();
    for button in [&fix, &not_now, &never] {
        button_row.add(button, 0, SizerFlag::All, 0);
    }
    sizer.add_sizer(&button_row, 0, SizerFlag::All, 5);
    dialog.set_sizer(sizer, true);

    const ID_NEVER: i32 = 6500;
    {
        let dialog = dialog;
        fix.on_click(move |_| dialog.end_modal(ID_OK));
    }
    {
        let dialog = dialog;
        not_now.on_click(move |_| dialog.end_modal(wxdragon::id::ID_CANCEL as i32));
    }
    {
        let dialog = dialog;
        never.on_click(move |_| dialog.end_modal(ID_NEVER));
    }

    match dialog.show_modal() {
        ID_OK => PortabilityChoice::Fix(
            report
                .fixes
                .iter()
                .enumerate()
                .filter(|(index, _)| list.is_checked(*index as u32))
                .map(|(_, fix)| fix.clone())
                .collect(),
        ),
        ID_NEVER => PortabilityChoice::NeverAsk,
        _ => PortabilityChoice::NotNow,
    }
}

/// One line of the merge list: what the command is, plus whatever the checks
/// turned up about it. Everything goes on the row itself rather than into a
/// details pane, because a screen reader reads the row and nothing else.
fn merge_row_label(candidate: &merge::Candidate) -> String {
    let mut label = format_args(
        &tr("{name} ({path})"),
        &[
            ("name", Arg::Str(&candidate.command.name)),
            ("path", Arg::Str(&candidate.command.path)),
        ],
    );
    for warning in &candidate.warnings {
        label.push_str(" - ");
        label.push_str(&merge_warning_text(warning));
    }
    label
}

fn merge_warning_text(warning: &merge::Warning) -> String {
    match warning {
        merge::Warning::ShortcutTaken { shortcut, owner } => format_args(
            &tr("the shortcut {shortcut} already belongs to {owner}, so this one is imported without it"),
            &[("shortcut", Arg::Str(shortcut)), ("owner", Arg::Str(owner))],
        ),
        merge::Warning::ShortcutTakenInFile { shortcut, owner } => format_args(
            &tr("the shortcut {shortcut} is also used by {owner} in this file, so this one is imported without it"),
            &[("shortcut", Arg::Str(shortcut)), ("owner", Arg::Str(owner))],
        ),
        merge::Warning::UnknownVariable { name } => format_args(
            &tr("there is no variable called {name} on this machine"),
            &[("name", Arg::Str(&portable::placeholder(name)))],
        ),
        merge::Warning::MissingPath { path } => format_args(
            &tr("{path} does not exist on this machine"),
            &[("path", Arg::Str(path))],
        ),
    }
}

/// Pick which of the new commands in another file to import. Returns the
/// ticked commands; empty when nothing was chosen or the dialog was cancelled.
///
/// Only commands this list does not already hold are ever offered (see
/// [`merge::plan`]), so there is no "replace" here and nothing to decide about
/// the commands already present — they are not in the list at all.
pub fn merge_dialog(parent: &Frame, file_name: &str, plan: &merge::Plan) -> Vec<Command> {
    let dialog = Dialog::builder(parent, &tr("Merge commands")).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();

    let mut summary = format_args(
        &tr("{count} new commands were found in {file}. Tick the ones to add. Nothing already in your list is changed, replaced or removed."),
        &[
            ("count", Arg::Int(plan.candidates.len() as i64)),
            ("file", Arg::Str(file_name)),
        ],
    );
    if plan.already_present > 0 {
        summary.push('\n');
        summary.push_str(&format_args(
            &tr("{count} commands in that file are already in your list and are not shown."),
            &[("count", Arg::Int(plan.already_present as i64))],
        ));
    }
    let help = StaticText::builder(&dialog).with_label(&summary).build();
    sizer.add(&help, 0, SizerFlag::All, 5);

    let list_title = tr("Commands to &import:");
    let list_label = StaticText::builder(&dialog).with_label(&list_title).build();
    sizer.add(&list_label, 0, SizerFlag::All, 5);
    let list = CheckListBox::builder(&dialog).build();
    ax_name(&list, &list_title);
    for candidate in &plan.candidates {
        list.append(&merge_row_label(candidate));
    }
    // The user browsed to this file on purpose and only new commands are on
    // offer, so "all of them" is the expected answer; the buttons below make
    // picking a few out of a long list cheap.
    for index in 0..list.get_count() {
        list.check(index, true);
    }
    sizer.add(&list, 1, SizerFlag::Expand, 5);

    let button_row = BoxSizer::builder(Orientation::Horizontal).build();
    let all = Button::builder(&dialog).with_label(&tr("Select &all")).build();
    let none = Button::builder(&dialog).with_label(&tr("Select &none")).build();
    let import = Button::builder(&dialog)
        .with_id(ID_OK)
        .with_label(&tr("Im&port selected"))
        .build();
    let cancel = Button::builder(&dialog)
        .with_id(wxdragon::id::ID_CANCEL)
        .with_label(&tr("&Cancel"))
        .build();
    import.set_default();
    for button in [&all, &none, &import, &cancel] {
        button_row.add(button, 0, SizerFlag::All, 0);
    }
    sizer.add_sizer(&button_row, 0, SizerFlag::All, 5);
    dialog.set_sizer(sizer, true);

    all.on_click(move |_| {
        for index in 0..list.get_count() {
            list.check(index, true);
        }
    });
    none.on_click(move |_| {
        for index in 0..list.get_count() {
            list.check(index, false);
        }
    });
    {
        let dialog = dialog;
        import.on_click(move |_| dialog.end_modal(ID_OK));
    }
    {
        let dialog = dialog;
        cancel.on_click(move |_| dialog.end_modal(wxdragon::id::ID_CANCEL as i32));
    }

    if dialog.show_modal() != ID_OK {
        return Vec::new();
    }
    plan.candidates
        .iter()
        .enumerate()
        .filter(|(index, _)| list.is_checked(*index as u32))
        .map(|(_, candidate)| candidate.command.clone())
        .collect()
}

/// Settings dialog; returns true when saved (caller refreshes sound enable).
pub fn settings_dialog(
    parent: &Frame,
    settings: &mut launchtype_core::settings::SettingsStore,
) -> bool {
    let dialog = Dialog::builder(parent, &tr("Settings")).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();

    let sounds_cb = checkbox(&dialog, &tr("Enable &sounds"));
    sounds_cb.set_value(settings.settings.enable_sounds);
    sizer.add(&sounds_cb, 0, SizerFlag::All, 5);
    let minimized_cb = checkbox(&dialog, &tr("Start &minimized"));
    minimized_cb.set_value(settings.settings.start_minimized);
    sizer.add(&minimized_cb, 0, SizerFlag::All, 5);
    let snippets_cb = checkbox(&dialog, &tr("Start in s&nippets mode when invoked"));
    snippets_cb.set_value(settings.settings.snippets_on_invoke);
    sizer.add(&snippets_cb, 0, SizerFlag::All, 5);
    let portability_cb =
        checkbox(&dialog, &tr("Check for machine-s&pecific paths at startup"));
    portability_cb.set_value(settings.settings.portability_check);
    sizer.add(&portability_cb, 0, SizerFlag::All, 5);

    // How long the encrypted vault stays open, and how long a secret it copied
    // is allowed to sit on the clipboard.
    let vault_lock_title = tr("Lock the &vault after this many idle minutes (0 = after every copy):");
    let vault_lock_label = StaticText::builder(&dialog).with_label(&vault_lock_title).build();
    let vault_lock_row = BoxSizer::builder(Orientation::Horizontal).build();
    let vault_lock_spin = SpinCtrl::builder(&dialog)
        .with_min_value(0)
        .with_max_value(1440)
        .with_initial_value(settings.settings.vault_lock_minutes.min(1440) as i32)
        .build();
    ax_name(&vault_lock_spin, &vault_lock_title);
    vault_lock_row.add(&vault_lock_label, 0, SizerFlag::All, 0);
    vault_lock_row.add(&vault_lock_spin, 0, SizerFlag::All, 0);
    sizer.add_sizer(&vault_lock_row, 0, SizerFlag::All, 5);

    let vault_clip_title =
        tr("Clear the clipboard of a copied secre&t after this many seconds (0 = never):");
    let vault_clip_label = StaticText::builder(&dialog).with_label(&vault_clip_title).build();
    let vault_clip_row = BoxSizer::builder(Orientation::Horizontal).build();
    let vault_clip_spin = SpinCtrl::builder(&dialog)
        .with_min_value(0)
        .with_max_value(3600)
        .with_initial_value(settings.settings.vault_clipboard_seconds.min(3600) as i32)
        .build();
    ax_name(&vault_clip_spin, &vault_clip_title);
    vault_clip_row.add(&vault_clip_label, 0, SizerFlag::All, 0);
    vault_clip_row.add(&vault_clip_spin, 0, SizerFlag::All, 0);
    sizer.add_sizer(&vault_clip_row, 0, SizerFlag::All, 5);

    let steam_label = StaticText::builder(&dialog).with_label(&tr("Steam &library path:")).build();
    sizer.add(&steam_label, 0, SizerFlag::All, 5);
    let steam_row = BoxSizer::builder(Orientation::Horizontal).build();
    let steam_entry = TextCtrl::builder(&dialog).build();
    ax_name(&steam_entry, &tr("Steam &library path:"));
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
    ax_name(&ai_choice, &tr("AI model for screenshot descriptions:"));
    ai_choice.append(&tr("Claude Opus (best quality)"));
    ai_choice.append(&tr("Claude Sonnet (balanced)"));
    ai_choice.append(&tr("Claude Haiku (fastest, lightest)"));
    let selected = AI_MODEL_IDS
        .iter()
        .position(|id| *id == settings.settings.ai_model)
        .unwrap_or(0);
    ai_choice.set_selection(selected as u32);
    sizer.add(&ai_choice, 0, SizerFlag::Expand, 5);

    let language_label = StaticText::builder(&dialog).with_label(&tr("&Language:")).build();
    sizer.add(&language_label, 0, SizerFlag::All, 5);
    let language_choice = Choice::builder(&dialog).build();
    ax_name(&language_choice, &tr("&Language:"));
    language_choice.append(&tr("Same as the system"));
    language_choice.append(&tr("English"));
    language_choice.append(&tr("Spanish"));
    let language_index = LANGUAGE_CODES
        .iter()
        .position(|code| *code == settings.settings.language)
        .unwrap_or(0);
    language_choice.set_selection(language_index as u32);
    sizer.add(&language_choice, 0, SizerFlag::Expand, 5);

    // Every commands-shaped JSON sitting next to the app, plus whatever is
    // configured, so a missing file is still shown rather than silently lost.
    let commands_label = StaticText::builder(&dialog).with_label(&tr("Co&mmands file:")).build();
    sizer.add(&commands_label, 0, SizerFlag::All, 5);
    let commands_combo = ComboBox::builder(&dialog).build();
    ax_name(&commands_combo, &tr("Co&mmands file:"));
    let mut found = launchtype_core::model::commands_files_in(std::path::Path::new("."));
    if !found.iter().any(|name| *name == settings.settings.commands_file) {
        found.insert(0, settings.settings.commands_file.clone());
    }
    for name in &found {
        commands_combo.append(name);
    }
    commands_combo.set_value(&settings.settings.commands_file);
    sizer.add(&commands_combo, 0, SizerFlag::Expand, 5);

    let ssh_label = StaticText::builder(&dialog)
        .with_label(&tr("SSH mode: key authentication is used when a key file is set."))
        .build();
    sizer.add(&ssh_label, 0, SizerFlag::All, 5);
    let ssh_host_entry = labeled_row(&dialog, &sizer, &tr("SSH &server:"));
    ssh_host_entry.set_value(&settings.settings.ssh_host);
    let ssh_port_row = BoxSizer::builder(Orientation::Horizontal).build();
    let ssh_port_label = StaticText::builder(&dialog).with_label(&tr("SSH p&ort:")).build();
    let ssh_port_spin = SpinCtrl::builder(&dialog)
        .with_min_value(1)
        .with_max_value(65535)
        .with_initial_value(settings.settings.ssh_port as i32)
        .build();
    ax_name(&ssh_port_spin, &tr("SSH p&ort:"));
    ssh_port_row.add(&ssh_port_label, 0, SizerFlag::All, 0);
    ssh_port_row.add(&ssh_port_spin, 0, SizerFlag::All, 0);
    sizer.add_sizer(&ssh_port_row, 0, SizerFlag::All, 5);
    let ssh_user_entry = labeled_row(&dialog, &sizer, &tr("SSH &user name:"));
    ssh_user_entry.set_value(&settings.settings.ssh_user);

    let ssh_key_row = BoxSizer::builder(Orientation::Horizontal).build();
    let ssh_key_label = StaticText::builder(&dialog)
        .with_label(&tr("SSH private &key file (optional):"))
        .build();
    let ssh_key_entry = TextCtrl::builder(&dialog).build();
    ax_name(&ssh_key_entry, &tr("SSH private &key file (optional):"));
    ssh_key_entry.set_value(&settings.settings.ssh_key_path);
    let ssh_key_browse = Button::builder(&dialog).with_label(&tr("B&rowse...")).build();
    ssh_key_row.add(&ssh_key_label, 0, SizerFlag::All, 0);
    ssh_key_row.add(&ssh_key_entry, 1, SizerFlag::Expand, 0);
    ssh_key_row.add(&ssh_key_browse, 0, SizerFlag::All, 0);
    sizer.add_sizer(&ssh_key_row, 0, SizerFlag::Expand, 5);

    let ssh_password_entry =
        labeled_password_row(&dialog, &sizer, &tr("SSH pass&word (or key passphrase):"));
    ssh_password_entry.set_value(&settings.settings.ssh_password);

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
                // The stored value may be a placeholder; the picker needs the
                // real folder to start in.
                &portable::expand(
                    &steam_entry.get_value(),
                    launchtype_services::portable::vars(),
                ),
            )
            .build();
            if dir_dialog.show_modal() == ID_OK {
                if let Some(path) = dir_dialog.get_path() {
                    steam_entry.set_value(&portabilize(&path));
                }
            }
        });
    }
    {
        let dialog = dialog;
        ssh_key_browse.on_click(move |_| {
            let file_dialog = FileDialog::builder(&dialog)
                .with_message(&tr("Choose an SSH private key file"))
                .with_wildcard(&tr("All files (*.*)|*.*"))
                .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
                .build();
            if file_dialog.show_modal() == ID_OK {
                if let Some(path) = file_dialog.get_path() {
                    ssh_key_entry.set_value(&portabilize(&path));
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

    let language_before = settings.settings.language.clone();
    settings.settings.enable_sounds = sounds_cb.get_value();
    settings.settings.start_minimized = minimized_cb.get_value();
    settings.settings.snippets_on_invoke = snippets_cb.get_value();
    settings.settings.portability_check = portability_cb.get_value();
    settings.settings.vault_lock_minutes = vault_lock_spin.value().clamp(0, 1440) as u32;
    settings.settings.vault_clipboard_seconds = vault_clip_spin.value().clamp(0, 3600) as u32;
    settings.settings.steam_library = steam_entry.get_value();
    let ai_index = ai_choice.get_selection().unwrap_or(0) as usize;
    settings.settings.ai_model = AI_MODEL_IDS[ai_index.min(AI_MODEL_IDS.len() - 1)].to_string();
    let language_index = language_choice.get_selection().unwrap_or(0) as usize;
    settings.settings.language =
        LANGUAGE_CODES[language_index.min(LANGUAGE_CODES.len() - 1)].to_string();
    // An empty box means "keep the current file" rather than "no commands".
    let commands_file = commands_combo.get_value().trim().to_string();
    if !commands_file.is_empty() {
        settings.settings.commands_file = commands_file;
    }
    settings.settings.ssh_host = ssh_host_entry.get_value().trim().to_string();
    settings.settings.ssh_port = ssh_port_spin.value().clamp(1, 65535) as u16;
    settings.settings.ssh_user = ssh_user_entry.get_value().trim().to_string();
    settings.settings.ssh_key_path = ssh_key_entry.get_value().trim().to_string();
    settings.settings.ssh_password = ssh_password_entry.get_value();
    let _ = settings.save();

    // The window and its labels were built in the old language, so a live
    // catalog swap would only half apply.
    if settings.settings.language != language_before {
        MessageDialog::builder(
            parent,
            &tr("The new language will be used the next time Launchtype starts."),
            &tr("Language changed"),
        )
        .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
        .build()
        .show_modal();
    }
    true
}

/// The sound row shared by the timer and alarm dialogs: a dropdown of the
/// tones bundled under `sounds/<category>/`, plus a "Custom file..." row backed
/// by the path entry and Browse. Each pick plays as it lands, so arrowing
/// through the dropdown auditions the tones without a separate Test button.
#[derive(Clone)]
struct SoundPicker {
    choice: Choice,
    entry: TextCtrl,
    /// Stored values, parallel to the dropdown rows after the leading
    /// "No sound"; the trailing "Custom file..." row has none, so an index
    /// past the end means "use whatever is in the entry".
    bundled: Vec<String>,
}

impl SoundPicker {
    /// What to store on the timer or alarm: empty for the system beep, a
    /// `<category>/<file>.wav` path for a bundled tone, or the browsed-for
    /// absolute path.
    fn value(&self) -> String {
        match self.choice.get_selection() {
            None | Some(0) => String::new(),
            Some(index) => self
                .bundled
                .get(index as usize - 1)
                .cloned()
                .unwrap_or_else(|| self.entry.get_value()),
        }
    }

    /// Audition the current pick, cutting off the one before it. Silent for
    /// "No sound" and for a custom path that is not there yet — arrowing onto
    /// either is routine, and a beep every time would only be noise.
    fn preview(&self, sounds: &SoundPlayer) {
        sounds.stop();
        let value = self.value();
        if !value.is_empty() {
            sounds.play_alert(&value);
        }
    }
}

fn sound_picker(
    dialog: &Dialog,
    sizer: &BoxSizer,
    category: &str,
    sounds: &Arc<SoundPlayer>,
    initial: &str,
) -> SoundPicker {
    let alerts = sounds.bundled_alerts(category);
    let bundled: Vec<String> = alerts.iter().map(|(_, value)| value.clone()).collect();

    let label = StaticText::builder(dialog).with_label(&tr("&Sound:")).build();
    sizer.add(&label, 0, SizerFlag::All, 0);
    let choice = Choice::builder(dialog).build();
    ax_name(&choice, &tr("&Sound:"));
    choice.append(&tr("No sound (system beep)"));
    for (name, _) in &alerts {
        choice.append(name);
    }
    choice.append(&tr("Custom file..."));
    let custom_index = alerts.len() as u32 + 1;
    // An edited alert reopens on what it was saved with: a bundled tone selects
    // its own row, anything else is a custom path.
    let selected = match bundled.iter().position(|value| value == initial) {
        Some(index) => index as u32 + 1,
        None if initial.is_empty() => 0,
        None => custom_index,
    };
    choice.set_selection(selected);
    sizer.add(&choice, 0, SizerFlag::Expand, 0);

    let row = BoxSizer::builder(Orientation::Horizontal).build();
    let file_label = StaticText::builder(dialog).with_label(&tr("Custom sound &file:")).build();
    let entry = TextCtrl::builder(dialog).build();
    ax_name(&entry, &tr("Custom sound &file:"));
    if selected == custom_index {
        entry.set_value(initial);
    }
    let browse = Button::builder(dialog).with_label(&tr("&Browse...")).build();
    row.add(&file_label, 0, SizerFlag::All, 0);
    row.add(&entry, 0, SizerFlag::All, 0);
    row.add(&browse, 0, SizerFlag::All, 0);
    sizer.add_sizer(&row, 0, SizerFlag::All, 0);

    {
        let dialog = *dialog;
        let picker = SoundPicker { choice, entry, bundled: bundled.clone() };
        let sounds = sounds.clone();
        browse.on_click(move |_| {
            let file_dialog = FileDialog::builder(&dialog)
                .with_message(&tr("Choose a sound file"))
                .with_wildcard(&tr("Sound files (*.wav)|*.wav|All files (*.*)|*.*"))
                .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
                .build();
            if file_dialog.show_modal() == ID_OK {
                if let Some(path) = file_dialog.get_path() {
                    entry.set_value(&path);
                    // Browsing only means anything on the custom row, so move
                    // the dropdown there instead of dropping the pick.
                    choice.set_selection(custom_index);
                    // set_selection does not raise a command event, so the
                    // browsed file has to be auditioned by hand.
                    picker.preview(&sounds);
                }
            }
        });
    }
    {
        let picker = SoundPicker { choice, entry, bundled: bundled.clone() };
        let sounds = sounds.clone();
        choice.on_selection_changed(move |_| picker.preview(&sounds));
    }

    SoundPicker { choice, entry, bundled }
}

/// Add/edit timer dialog; writes to the store on OK. `existing` seeds the
/// fields when editing. Returns true when the store changed.
pub fn timer_dialog(
    parent: &Frame,
    controller: &mut ModeController,
    existing: Option<TimerDef>,
) -> bool {
    let title = if existing.is_some() { tr("Edit Timer") } else { tr("Add Timer") };
    let dialog = Dialog::builder(parent, &title).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let title_entry = labeled_row(&dialog, &sizer, &tr("&Title:"));
    let desc_entry = labeled_row(&dialog, &sizer, &tr("&Description:"));

    let minutes_row = BoxSizer::builder(Orientation::Horizontal).build();
    let minutes_label = StaticText::builder(&dialog).with_label(&tr("&Minutes:")).build();
    let minutes_spin = SpinCtrl::builder(&dialog)
        .with_min_value(1)
        .with_max_value(1440)
        .with_initial_value(existing.as_ref().map_or(5, |t| t.minutes.clamp(1, 1440) as i32))
        .build();
    ax_name(&minutes_spin, &tr("&Minutes:"));
    minutes_row.add(&minutes_label, 0, SizerFlag::All, 0);
    minutes_row.add(&minutes_spin, 0, SizerFlag::All, 0);
    sizer.add_sizer(&minutes_row, 0, SizerFlag::All, 0);

    let repeating_cb = checkbox(&dialog, &tr("&Repeating (fires every X minutes until disabled)"));
    sizer.add(&repeating_cb, 0, SizerFlag::All, 0);
    if let Some(seed) = &existing {
        title_entry.set_value(&seed.title);
        desc_entry.set_value(&seed.description);
        repeating_cb.set_value(seed.repeating);
    }
    let seeded_sound = existing.as_ref().and_then(|t| t.sound.clone()).unwrap_or_default();
    let sound = sound_picker(&dialog, &sizer, TIMER_SOUNDS, &controller.sounds, &seeded_sound);
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

    let confirmed = dialog.show_modal() == ID_OK;
    // A tone auditioned in the dropdown runs for seconds; don't let it outlive
    // the dialog it was picked in.
    controller.sounds.stop();
    if !confirmed {
        return false;
    }
    let sound = Some(sound.value());
    let minutes = minutes_spin.value().max(1) as u64;
    match existing {
        Some(seed) => controller.timers.update(
            TimerDef {
                title: title_entry.get_value(),
                description: desc_entry.get_value(),
                minutes,
                repeating: repeating_cb.get_value(),
                sound,
                ..seed
            },
            controller.clock.now(),
        ),
        None => {
            controller.timers.add(
                TimerDef::new(
                    title_entry.get_value(),
                    desc_entry.get_value(),
                    minutes,
                    repeating_cb.get_value(),
                    sound,
                ),
                controller.clock.now(),
            );
            true
        }
    }
}

/// Add/edit alarm dialog; writes to the store on OK. `existing` seeds the
/// fields when editing. Returns true when the store changed.
pub fn alarm_dialog(
    parent: &Frame,
    controller: &mut ModeController,
    existing: Option<AlarmDef>,
) -> bool {
    let title = if existing.is_some() { tr("Edit Alarm") } else { tr("Add Alarm") };
    let dialog = Dialog::builder(parent, &title).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let title_entry = labeled_row(&dialog, &sizer, &tr("&Title:"));
    let desc_entry = labeled_row(&dialog, &sizer, &tr("&Description:"));

    let hour_row = BoxSizer::builder(Orientation::Horizontal).build();
    let hour_label = StaticText::builder(&dialog).with_label(&tr("&Hour (0-23):")).build();
    let hour_spin = SpinCtrl::builder(&dialog)
        .with_min_value(0)
        .with_max_value(23)
        .with_initial_value(existing.as_ref().map_or(8, |a| a.hour.min(23) as i32))
        .build();
    ax_name(&hour_spin, &tr("&Hour (0-23):"));
    hour_row.add(&hour_label, 0, SizerFlag::All, 0);
    hour_row.add(&hour_spin, 0, SizerFlag::All, 0);
    sizer.add_sizer(&hour_row, 0, SizerFlag::All, 0);

    let minute_row = BoxSizer::builder(Orientation::Horizontal).build();
    let minute_label = StaticText::builder(&dialog).with_label(&tr("&Minute (0-59):")).build();
    let minute_spin = SpinCtrl::builder(&dialog)
        .with_min_value(0)
        .with_max_value(59)
        .with_initial_value(existing.as_ref().map_or(0, |a| a.minute.min(59) as i32))
        .build();
    ax_name(&minute_spin, &tr("&Minute (0-59):"));
    minute_row.add(&minute_label, 0, SizerFlag::All, 0);
    minute_row.add(&minute_spin, 0, SizerFlag::All, 0);
    sizer.add_sizer(&minute_row, 0, SizerFlag::All, 0);

    if let Some(seed) = &existing {
        title_entry.set_value(&seed.title);
        desc_entry.set_value(&seed.description);
    }
    let seeded_sound = existing.as_ref().and_then(|a| a.sound.clone()).unwrap_or_default();
    let sound = sound_picker(&dialog, &sizer, ALARM_SOUNDS, &controller.sounds, &seeded_sound);
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

    let confirmed = dialog.show_modal() == ID_OK;
    // A tone auditioned in the dropdown runs for seconds; don't let it outlive
    // the dialog it was picked in.
    controller.sounds.stop();
    if !confirmed {
        return false;
    }
    let sound = Some(sound.value());
    let hour = hour_spin.value().clamp(0, 23) as u32;
    let minute = minute_spin.value().clamp(0, 59) as u32;
    match existing {
        Some(seed) => controller.alarms.update(AlarmDef {
            title: title_entry.get_value(),
            description: desc_entry.get_value(),
            hour,
            minute,
            sound,
            ..seed
        }),
        None => {
            controller.alarms.add(AlarmDef::new(
                title_entry.get_value(),
                desc_entry.get_value(),
                hour,
                minute,
                sound,
            ));
            true
        }
    }
}

/// Add/edit snippet dialog. `existing` = (shortcut, contents) when editing.
/// Returns true when saved (caller reloads snippets).
pub fn snippet_dialog(parent: &Frame, existing: Option<(String, String)>) -> bool {
    let title = if existing.is_some() { tr("Edit Snippet") } else { tr("Add Snippet") };
    let dialog = Dialog::builder(parent, &title).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let name_entry = labeled_row(&dialog, &sizer, &tr("Name:"));
    let contents_label = StaticText::builder(&dialog).with_label(&tr("Contents:")).build();
    sizer.add(&contents_label, 0, SizerFlag::All, 0);
    let contents_entry = TextCtrl::builder(&dialog)
        .with_style(wxdragon::widgets::textctrl::TextCtrlStyle::MultiLine)
        .build();
    ax_name(&contents_entry, &tr("Contents:"));
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

/// Ask for the master password to open the vault. `None` when cancelled.
pub fn vault_unlock_dialog(parent: &Frame) -> Option<String> {
    let dialog = Dialog::builder(parent, &tr("Unlock the vault")).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let entry = labeled_password_row(&dialog, &sizer, &tr("&Master password:"));
    let (ok, cancel) = ok_cancel_row(&dialog, &sizer);
    dialog.set_sizer(sizer, true);

    {
        let dialog = dialog;
        ok.on_click(move |_| {
            if entry.get_value().is_empty() {
                error_box(&dialog, &tr("Please enter your master password."), &tr("Error"));
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
    Some(entry.get_value())
}

/// The master password the user chose, plus the current one when they were
/// asked to prove they knew it.
pub struct VaultPasswords {
    pub current: String,
    pub new: String,
}

/// Choose a master password: for a vault that does not exist yet
/// (`changing` = false), or to replace the one it has.
///
/// The confirmation field is not ceremony here — there is no reset link and no
/// recovery, so a password typed wrong once is a vault nobody can open again.
pub fn vault_password_dialog(parent: &Frame, changing: bool) -> Option<VaultPasswords> {
    let title = if changing { tr("Change the master password") } else { tr("Set up the vault") };
    let help = if changing {
        tr("The entries themselves are not re-encrypted, so this is instant. Everything in the vault will need the new password from now on.")
    } else {
        tr("This password encrypts everything you put in the vault, and it is the only way back in: it is not stored anywhere and cannot be recovered or reset. Keep the whole vault folder together when you back it up or move Launchtype elsewhere.")
    };
    let dialog = Dialog::builder(parent, &title).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let help_label = StaticText::builder(&dialog).with_label(&help).build();
    sizer.add(&help_label, 0, SizerFlag::All, 5);

    let current_entry = changing
        .then(|| labeled_password_row(&dialog, &sizer, &tr("C&urrent master password:")));
    let new_entry = labeled_password_row(&dialog, &sizer, &tr("&New master password:"));
    let confirm_entry = labeled_password_row(&dialog, &sizer, &tr("&Type it again:"));
    let (ok, cancel) = ok_cancel_row(&dialog, &sizer);
    dialog.set_sizer(sizer, true);

    {
        let dialog = dialog;
        ok.on_click(move |_| {
            if current_entry.is_some_and(|entry| entry.get_value().is_empty()) {
                error_box(&dialog, &tr("Please enter your current master password."), &tr("Error"));
                return;
            }
            let new = new_entry.get_value();
            if new.chars().count() < launchtype_core::vault::MIN_PASSWORD_LEN {
                error_box(
                    &dialog,
                    &format_args(
                        &tr("The master password must be at least {count} characters long."),
                        &[("count", Arg::Int(launchtype_core::vault::MIN_PASSWORD_LEN as i64))],
                    ),
                    &tr("Error"),
                );
                return;
            }
            if new != confirm_entry.get_value() {
                error_box(&dialog, &tr("The two passwords do not match."), &tr("Error"));
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
    Some(VaultPasswords {
        current: current_entry.map(|entry| entry.get_value()).unwrap_or_default(),
        new: new_entry.get_value(),
    })
}

/// What one vault entry holds, as the dialog reads and writes it.
#[derive(Default, Clone)]
pub struct VaultEntryFields {
    pub name: String,
    pub shortcut: String,
    pub secret: String,
}

/// Add or edit a vault entry. `existing` seeds the fields when editing.
///
/// The secret is shown in the clear rather than masked. A masked field is read
/// out by a screen reader as nothing at all, which would leave no way to check
/// what was typed or what is stored — and the vault is already open by the
/// time this dialog is up, so masking here would buy shoulder-surfing cover
/// and nothing else. It is multi-line so recovery codes and keys fit.
pub fn vault_entry_dialog(parent: &Frame, existing: Option<VaultEntryFields>) -> Option<VaultEntryFields> {
    let title = if existing.is_some() { tr("Edit vault entry") } else { tr("Add to the vault") };
    let dialog = Dialog::builder(parent, &title).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let name_entry = labeled_row(&dialog, &sizer, &tr("&Name:"));
    let shortcut_entry = labeled_row(&dialog, &sizer, &tr("&Shortcut (optional):"));
    let secret_title = tr("Sec&ret (shown here, never written to disk unencrypted):");
    let secret_label = StaticText::builder(&dialog).with_label(&secret_title).build();
    sizer.add(&secret_label, 0, SizerFlag::All, 0);
    let secret_entry = TextCtrl::builder(&dialog)
        .with_style(wxdragon::widgets::textctrl::TextCtrlStyle::MultiLine)
        .build();
    ax_name(&secret_entry, &secret_title);
    sizer.add(&secret_entry, 1, SizerFlag::Expand, 0);
    let (ok, cancel) = ok_cancel_row(&dialog, &sizer);
    dialog.set_sizer(sizer, true);

    if let Some(seed) = &existing {
        name_entry.set_value(&seed.name);
        shortcut_entry.set_value(&seed.shortcut);
        secret_entry.set_value(&seed.secret);
    }

    {
        let dialog = dialog;
        ok.on_click(move |_| {
            if name_entry.get_value().trim().is_empty() || secret_entry.get_value().is_empty() {
                error_box(
                    &dialog,
                    &tr("Please enter a name and a secret for this entry."),
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
    Some(VaultEntryFields {
        name: name_entry.get_value().trim().to_string(),
        shortcut: shortcut_entry.get_value().trim().to_lowercase(),
        secret: secret_entry.get_value(),
    })
}

/// Warn before setting up a vault on top of encrypted files whose key file is
/// missing. No password opens those any more, and a new vault would leave them
/// sitting there unreadable — which is worth saying out loud before it happens
/// rather than after.
pub fn confirm_vault_orphans(parent: &Frame, count: usize) -> bool {
    question_box(
        parent,
        &format_args(
            &tr("The vault folder holds {count} encrypted files, but the key file that goes with them is missing, so nothing can open them any more. Set up a new, empty vault anyway?"),
            &[("count", Arg::Int(count as i64))],
        ),
        &tr("Vault key file missing"),
    )
}

/// Confirm a deletion that cannot be undone: the entry is the only copy of
/// whatever it holds, and nothing about it is recoverable once the file is gone.
pub fn confirm_vault_delete(parent: &Frame, name: &str) -> bool {
    question_box(
        parent,
        &format_args(
            &tr("Delete {name} from the vault? The secret in it cannot be recovered afterwards."),
            &[("name", Arg::Str(name))],
        ),
        &tr("Delete vault entry"),
    )
}

/// Ask what to crop out of the next screenshot; `None` when the user cancels.
///
/// The request used to be typed into the launcher's own input field, so the
/// same box both filtered the action list and carried the sentence sent to the
/// AI — every character typed reshuffled the list under the user. A dialog
/// keeps the two apart and gives the request a label that says what it is for.
pub fn grab_region_dialog(parent: &Frame) -> Option<String> {
    let dialog = Dialog::builder(parent, &tr("Grab a specific region")).build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let help = StaticText::builder(&dialog)
        .with_label(&tr(
            "Describe the part of the screen you want cropped out, for example \"the OK button\" or \"the error message\".",
        ))
        .build();
    sizer.add(&help, 0, SizerFlag::All, 5);
    let entry = labeled_row(&dialog, &sizer, &tr("&What to grab:"));
    let (ok, cancel) = ok_cancel_row(&dialog, &sizer);
    dialog.set_sizer(sizer, true);

    {
        let dialog = dialog;
        ok.on_click(move |_| {
            if entry.get_value().trim().is_empty() {
                error_box(&dialog, &tr("Please describe what you want to grab."), &tr("Error"));
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
    Some(entry.get_value().trim().to_string())
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
