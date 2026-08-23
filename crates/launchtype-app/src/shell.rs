//! The main window — port of `managers/ui_manager.py`: input field, results
//! list, button row, mode switching by trigger character, spoken result
//! announcements, and the Run dispatch.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use launchtype_core::i18n::{format_args, tr, Arg};
use launchtype_core::mode::UiMode;
use launchtype_core::settings::SettingsStore;
use launchtype_services::poller::ClipboardPoller;
use launchtype_services::runner::run_command;
use launchtype_services::scheduler::Scheduler;
use launchtype_services::sounds::SoundPlayer;
use launchtype_services::ssh::SshSession;
use launchtype_services::vault::VaultLocker;
use launchtype_services::{apps, clipboard, notebrook, steam};
use wxdragon::dialogs::file_dialog::{FileDialog, FileDialogStyle};
use wxdragon::prelude::*;

use crate::controller::{Item, ItemKind, ModeController};
use crate::speech::speak_now;

/// Notes are always posted to this channel (created on demand).
const NOTEBROOK_CHANNEL: &str = "feeds";

pub struct Shell {
    pub frame: Frame,
    pub panel: Panel,
    pub edit: TextCtrl,
    sort_label: StaticText,
    sort_choice: Choice,
    /// Commands-mode only, like the sort control: opens a terminal at the
    /// folder the focused command's arguments point at.
    terminal_button: Button,
    /// Copies a command's arguments, or an app's program file — kept here so
    /// its label can say which (see [`update_list`]).
    copy_args_button: Button,
    pub list: ListBox,
    pub mode: UiMode,
    pub items: Vec<Item>,
    pub controller: ModeController,
    pub settings: SettingsStore,
    pub sounds: Arc<SoundPlayer>,
    cli_snippets_on_invoke: bool,
    pub cli_quiet: bool,
    /// `-c/--commands` was given, so the Settings commands-file picker must
    /// not fight the command line for the rest of this run.
    pub commands_file_from_cli: bool,
    /// Live SSH connection for `$` mode, kept between commands.
    pub ssh: Option<SshSession>,
    /// A command is in flight; the next Enter is ignored rather than queued.
    pub ssh_busy: bool,
    pub poller: Option<ClipboardPoller>,
    pub scheduler: Option<Scheduler>,
    /// Wipes the vault key once it has gone unused for the configured time.
    pub vault_locker: Option<VaultLocker>,
    /// Transient "explore regions" state: the full-resolution capture and
    /// the size it was sent to the AI at (region boxes are in that space).
    pub screenshot_image: Option<launchtype_services::screenshot::RgbaImage>,
    pub screenshot_sent_size: Option<(u32, u32)>,
}

pub type SharedShell = Rc<RefCell<Shell>>;

thread_local! {
    static ACTIVE_SHELL: RefCell<Option<SharedShell>> = const { RefCell::new(None) };
}

/// Register the shell for cross-thread completions (main thread only).
/// Background workers marshal back with `wxdragon::call_after` and reach the
/// shell through [`with_shell`], because `Rc` handles cannot cross threads.
pub fn set_active_shell(shell: &SharedShell) {
    ACTIVE_SHELL.with(|slot| *slot.borrow_mut() = Some(shell.clone()));
}

pub fn with_shell(f: impl FnOnce(&SharedShell)) {
    ACTIVE_SHELL.with(|slot| {
        if let Some(shell) = slot.borrow().clone() {
            f(&shell);
        }
    });
}

impl Shell {
    fn snippets_on_invoke(&self) -> bool {
        self.cli_snippets_on_invoke || self.settings.settings.snippets_on_invoke
    }
}

pub fn build_shell(
    controller: ModeController,
    settings: SettingsStore,
    sounds: Arc<SoundPlayer>,
    cli_snippets_on_invoke: bool,
    cli_quiet: bool,
    commands_file_from_cli: bool,
) -> SharedShell {
    let frame = Frame::builder().with_title("Launchtype").build();
    let panel = Panel::builder(&frame).build();

    let sizer = BoxSizer::builder(Orientation::Vertical).build();

    let edit_sizer = BoxSizer::builder(Orientation::Horizontal).build();
    let edit_label = StaticText::builder(&panel).with_label(&tr("Input Field")).build();
    let edit = TextCtrl::builder(&panel).build();
    // wxUSE_ACCESSIBILITY makes the generic accessible report the control's type
    // name ("text") instead of this label, so name it explicitly.
    edit.set_name(&tr("Input Field"));
    edit_sizer.add(&edit_label, 0, SizerFlag::All, 0);
    edit_sizer.add(&edit, 0, SizerFlag::All, 0);
    sizer.add_sizer(&edit_sizer, 0, SizerFlag::All, 0);

    // Deliberately unlabelled: a name here only made the screen reader say
    // "Results" before every item, which slows down arrowing through the list.
    // wxUSE_ACCESSIBILITY falls back to the control's type name, not to a
    // neighbouring static text, so dropping the label is safe.
    let list = ListBox::builder(&panel).build();
    sizer.add(&list, 0, SizerFlag::All, 0);

    // Commands-mode sort order, placed after the list. Only shown in commands
    // mode (see update_list).
    let sort_sizer = BoxSizer::builder(Orientation::Horizontal).build();
    let sort_label = StaticText::builder(&panel).with_label(&tr("Sort commands by:")).build();
    let sort_choice = Choice::builder(&panel).build();
    sort_choice.set_name(&tr("Sort commands by:"));
    sort_choice.append(&tr("Last modified"));
    sort_choice.append(&tr("Number of uses"));
    sort_choice.set_selection(if settings.settings.command_sort_by_uses { 1 } else { 0 });
    sort_sizer.add(&sort_label, 0, SizerFlag::All, 0);
    sort_sizer.add(&sort_choice, 0, SizerFlag::All, 0);
    sizer.add_sizer(&sort_sizer, 0, SizerFlag::All, 0);

    let button_sizer = BoxSizer::builder(Orientation::Horizontal).build();
    let add_button = Button::builder(&panel).with_label(&tr("&Add...")).build();
    let edit_button = Button::builder(&panel).with_label(&tr("&Edit...")).build();
    let copy_button = Button::builder(&panel).with_label(&tr("&COPY...")).build();
    let delete_button = Button::builder(&panel).with_label(&tr("&Delete")).build();
    let copy_args_button =
        Button::builder(&panel).with_label(&copy_args_label(UiMode::Commands)).build();
    let terminal_button =
        Button::builder(&panel).with_label(&tr("Open in &Terminal (Alt+T)")).build();
    let snippets_button = Button::builder(&panel).with_label(&tr("Open &Snippets folder")).build();
    let new_snippet_button = Button::builder(&panel).with_label(&tr("&New snipet")).build();
    let run_button = Button::builder(&panel).with_label(&tr("&Run")).build();
    let modes_button = Button::builder(&panel).with_label(&tr("&Modes (Alt+M)")).build();
    let help_button = Button::builder(&panel).with_label(&tr("&Help")).build();
    // "Sett&ings", not "Se&ttings": Alt+T belongs to the terminal button.
    let settings_button = Button::builder(&panel).with_label(&tr("Sett&ings...")).build();
    let merge_button = Button::builder(&panel).with_label(&tr("Mer&ge in...")).build();
    let exit_button = Button::builder(&panel).with_label(&tr("E&xit")).build();
    for b in [
        &add_button, &edit_button, &copy_button, &delete_button, &copy_args_button,
        &terminal_button, &snippets_button, &new_snippet_button, &run_button, &modes_button,
        &help_button, &settings_button, &merge_button, &exit_button,
    ] {
        button_sizer.add(b, 0, SizerFlag::All, 0);
    }
    run_button.set_default();
    sizer.add_sizer(&button_sizer, 0, SizerFlag::All, 0);

    panel.set_sizer(sizer, true);

    let shell: SharedShell = Rc::new(RefCell::new(Shell {
        frame,
        panel,
        edit,
        sort_label,
        sort_choice,
        terminal_button,
        copy_args_button,
        list,
        mode: UiMode::Commands,
        items: Vec::new(),
        controller,
        settings,
        sounds,
        cli_snippets_on_invoke,
        cli_quiet,
        commands_file_from_cli,
        ssh: None,
        ssh_busy: false,
        poller: None,
        scheduler: None,
        vault_locker: None,
        screenshot_image: None,
        screenshot_sent_size: None,
    }));

    bind_events(
        &shell,
        [
            add_button, edit_button, copy_button, delete_button, copy_args_button,
            terminal_button, snippets_button, new_snippet_button, run_button, modes_button,
            help_button, settings_button, merge_button, exit_button,
        ],
    );
    shell
}

fn bind_events(shell: &SharedShell, buttons: [Button; 14]) {
    let [add_button, edit_button, copy_button, delete_button, copy_args_button, terminal_button, snippets_button, new_snippet_button, run_button, modes_button, help_button, settings_button, merge_button, exit_button] =
        buttons;
    let (frame, edit, list, panel, sort_choice) = {
        let s = shell.borrow();
        (s.frame, s.edit, s.list, s.panel, s.sort_choice)
    };

    {
        let shell = shell.clone();
        edit.on_text_changed(move |_| update_list(&shell));
    }
    // Down/Up in the input field jump straight into the results: focus the
    // list on the entry after (Down) or before (Up) the selected one, so the
    // second result is one keystroke away instead of Tab-then-arrow.
    {
        let shell = shell.clone();
        edit.on_key_down(move |event| {
            let key = match &event {
                WindowEventData::Keyboard(key_event) => key_event.get_key_code(),
                _ => None,
            };
            let step: i64 = match key {
                Some(WXK_UP) => -1,
                Some(WXK_DOWN) => 1,
                _ => {
                    event.skip(true);
                    return;
                }
            };
            let s = shell.borrow();
            let count = s.list.get_count() as i64;
            if count == 0 {
                return;
            }
            let current = s.list.get_selection().map_or(-1, |index| index as i64);
            let target = (current + step).clamp(0, count - 1);
            s.list.set_selection(target as u32, true);
            s.list.set_focus();
        });
    }
    {
        let shell = shell.clone();
        sort_choice.on_selection_changed(move |_| {
            {
                let mut s = shell.borrow_mut();
                let by_uses = s.sort_choice.get_selection() == Some(1);
                s.controller.sort_by_uses = by_uses;
                s.settings.settings.command_sort_by_uses = by_uses;
                let _ = s.settings.save();
            }
            update_list(&shell);
        });
    }
    {
        let shell = shell.clone();
        run_button.on_click(move |_| run_clicked(&shell));
    }
    {
        let shell = shell.clone();
        add_button.on_click(move |_| {
            // The vault's dialogs borrow the shell for themselves, so that
            // branch has to run before the borrow below is taken.
            if shell.borrow().mode == UiMode::Vault {
                shell.borrow().edit.change_value("");
                crate::vault_flows::add_entry(&shell);
                update_list(&shell);
                return;
            }
            {
                let mut s = shell.borrow_mut();
                let frame = s.frame;
                match s.mode {
                    UiMode::Timers => {
                        crate::dialogs::timer_dialog(&frame, &mut s.controller, None);
                    }
                    UiMode::Alarms => {
                        crate::dialogs::alarm_dialog(&frame, &mut s.controller, None);
                    }
                    _ => {
                        crate::dialogs::command_edition_dialog(&frame, &mut s.controller, None);
                    }
                }
                s.edit.change_value("");
            }
            update_list(&shell);
        });
    }
    {
        let shell = shell.clone();
        edit_button.on_click(move |_| {
            let selected = {
                let s = shell.borrow();
                s.list.get_selection().and_then(|index| s.items.get(index as usize).cloned())
            };
            let Some(item) = selected else { return };
            // As with Add: the vault opens its own dialogs and borrows the
            // shell to do it.
            if matches!(item.kind, ItemKind::VaultEntry) {
                shell.borrow().edit.change_value("");
                crate::vault_flows::edit_entry(&shell, &item.id);
                update_list(&shell);
                return;
            }
            {
                let mut s = shell.borrow_mut();
                let frame = s.frame;
                match &item.kind {
                    ItemKind::Snippet => {
                        if crate::dialogs::snippet_dialog(
                            &frame,
                            Some((item.shortcut.clone(), item.name.clone())),
                        ) {
                            s.controller.reload_snippets();
                        }
                    }
                    // Timers and alarms live in their own stores, not in
                    // commands.json; the engine lock is released with the
                    // `let` so the dialog can take it again to save.
                    ItemKind::Timer => {
                        let seed = s
                            .controller
                            .timers
                            .engine
                            .lock()
                            .unwrap()
                            .timers
                            .iter()
                            .find(|t| t.id == item.id)
                            .cloned();
                        if let Some(seed) = seed {
                            crate::dialogs::timer_dialog(&frame, &mut s.controller, Some(seed));
                        }
                    }
                    ItemKind::Alarm => {
                        let seed = s
                            .controller
                            .alarms
                            .engine
                            .lock()
                            .unwrap()
                            .alarms
                            .iter()
                            .find(|a| a.id == item.id)
                            .cloned();
                        if let Some(seed) = seed {
                            crate::dialogs::alarm_dialog(&frame, &mut s.controller, Some(seed));
                        }
                    }
                    _ => {
                        let seed = s
                            .controller
                            .commands
                            .file
                            .commands
                            .iter()
                            .find(|c| c.id == item.id)
                            .cloned();
                        if let Some(seed) = seed {
                            crate::dialogs::command_edition_dialog(&frame, &mut s.controller, Some(seed));
                        }
                    }
                }
                s.edit.change_value("");
            }
            update_list(&shell);
        });
    }
    {
        let shell = shell.clone();
        copy_button.on_click(move |_| {
            {
                let mut s = shell.borrow_mut();
                let Some(index) = s.list.get_selection() else { return };
                let Some(item) = s.items.get(index as usize).cloned() else { return };
                let frame = s.frame;
                let seed = s
                    .controller
                    .commands
                    .file
                    .commands
                    .iter()
                    .find(|c| c.id == item.id)
                    .cloned();
                if let Some(mut seed) = seed {
                    // A copy starts without the display name and the shortcut.
                    seed.name = String::new();
                    seed.shortcut = Some(String::new());
                    crate::dialogs::command_edition_dialog(&frame, &mut s.controller, Some(seed));
                }
                s.edit.change_value("");
            }
            update_list(&shell);
        });
    }
    {
        let shell = shell.clone();
        new_snippet_button.on_click(move |_| {
            {
                let mut s = shell.borrow_mut();
                let frame = s.frame;
                if crate::dialogs::snippet_dialog(&frame, None) {
                    s.controller.reload_snippets();
                }
                s.edit.change_value("");
            }
            update_list(&shell);
            toggle_visibility(&shell);
        });
    }
    {
        let shell = shell.clone();
        settings_button.on_click(move |_| {
            {
                let mut s = shell.borrow_mut();
                let frame = s.frame;
                let before = s.settings.settings.clone();
                if !crate::dialogs::settings_dialog(&frame, &mut s.settings) {
                    return;
                }
                let after = s.settings.settings.clone();
                s.sounds.set_enabled(after.enable_sounds && !s.cli_quiet);
                // A shorter vault timeout has to bind to a session that is
                // already unlocked, not just to the next one.
                s.controller
                    .vault
                    .lock()
                    .unwrap()
                    .set_lock_after_minutes(after.vault_lock_minutes);
                // A different commands file takes effect immediately, unless
                // -c pinned one for this run.
                if after.commands_file != before.commands_file && !s.commands_file_from_cli {
                    s.controller.commands =
                        launchtype_services::stores::CommandsStore::load(&after.commands_file);
                    speak_now(
                        &format_args(
                            &tr("Now using {file}"),
                            &[("file", Arg::Str(&after.commands_file))],
                        ),
                        true,
                    );
                }
                // Re-point SSH mode at the new server on its next use.
                if crate::ssh_flows::config_changed(&before, &after) {
                    s.ssh = None;
                    s.ssh_busy = false;
                    s.controller.ssh_output.clear();
                }
            }
            update_list(&shell);
        });
    }
    {
        let shell = shell.clone();
        delete_button.on_click(move |_| {
            let selected = {
                let s = shell.borrow();
                s.list.get_selection().and_then(|index| s.items.get(index as usize).cloned())
            };
            let Some(item) = selected else { return };
            // Deleting a secret is the one deletion here that asks first, and
            // it opens a dialog to do it.
            if matches!(item.kind, ItemKind::VaultEntry) {
                crate::vault_flows::delete_entry(&shell, &item.id, &item.name);
                return;
            }
            {
                let mut s = shell.borrow_mut();
                // The id may belong to a command, a timer or an alarm.
                if !s.controller.commands.pop_by_uuid(&item.id) {
                    s.controller.timers.remove(&item.id);
                    s.controller.alarms.remove(&item.id);
                }
            }
            update_list(&shell);
        });
    }
    {
        let shell = shell.clone();
        copy_args_button.on_click(move |_| {
            let s = shell.borrow();
            let Some(index) = s.list.get_selection() else { return };
            let Some(item) = s.items.get(index as usize) else { return };
            match &item.kind {
                ItemKind::Command { args, .. } if !args.is_empty() => {
                    clipboard::set_text(args);
                    s.sounds.play("copy");
                    speak_now(&tr("Arguments copied"), true);
                }
                // An installed app has no arguments to copy, but it does have
                // the thing this button is reached for in commands mode: a
                // path worth pasting somewhere. Speak it as well as copying
                // it — the point is usually to check what it is.
                ItemKind::App { target } => match apps::executable_path(target) {
                    Some(path) => {
                        clipboard::set_text(&path);
                        s.sounds.play("copy");
                        speak_now(
                            &format_args(&tr("{path} copied"), &[("path", Arg::Str(&path))]),
                            true,
                        );
                    }
                    None => speak_now(&tr("This app has no program file to copy"), true),
                },
                _ => speak_now(&tr("No arguments"), true),
            }
        });
    }
    {
        let shell = shell.clone();
        terminal_button.on_click(move |_| open_in_terminal(&shell));
    }
    {
        let shell = shell.clone();
        snippets_button.on_click(move |_| {
            toggle_visibility(&shell);
            let _ = open::that_detached(std::env::current_dir().unwrap_or_default().join("snippets"));
        });
    }
    {
        let shell = shell.clone();
        help_button.on_click(move |_| {
            show_alert(
                &shell.borrow().frame,
                &tr("information"),
                &tr("The documentation will now open in your web browser."),
            );
            let _ = open::that_detached(tr("https://github.com/ogomez92/launchtype/blob/main/README.md"));
        });
    }
    {
        let shell = shell.clone();
        modes_button.on_click(move |_| show_modes_menu(&shell));
    }
    // The modes menu posts a wxEVT_MENU to the frame; route the selected id
    // back to its mode. Bound once, it also serves any future frame menus.
    {
        let shell = shell.clone();
        frame.on_menu(move |event| {
            let id = event.get_id();
            if let Some(index) = id.checked_sub(MODE_MENU_BASE_ID) {
                if let Some(&mode) = UiMode::MENU_MODES.get(index as usize) {
                    switch_to_mode(&shell, mode);
                }
            }
        });
    }
    {
        let shell = shell.clone();
        merge_button.on_click(move |_| merge_clicked(&shell));
    }
    {
        let shell = shell.clone();
        exit_button.on_click(move |_| exit_app(&shell));
    }

    // Escape hides the window instead of closing the app. wxWidgets does not
    // bubble key events up to parent windows, and wxdragon exposes neither an
    // accelerator table nor CHAR_HOOK, so the handler has to sit on every
    // control that can hold the focus — otherwise Escape is ignored (and the
    // native control beeps) whenever focus isn't in the input field.
    bind_hide_on_escape(shell, &frame);
    bind_hide_on_escape(shell, &panel);
    bind_hide_on_escape(shell, &edit);
    bind_hide_on_escape(shell, &list);
    bind_hide_on_escape(shell, &sort_choice);
    for button in [
        &add_button, &edit_button, &copy_button, &delete_button, &copy_args_button,
        &terminal_button, &snippets_button, &new_snippet_button, &run_button, &modes_button,
        &help_button, &settings_button, &merge_button, &exit_button,
    ] {
        bind_hide_on_escape(shell, button);
    }

    // Alt+F4 and the title-bar close box send a vetoable close event; hide the
    // window instead of quitting. A genuine exit (exit_app) forces the close
    // with `close(true)`, which is not vetoable, so it falls through.
    {
        let shell = shell.clone();
        frame.on_close(move |event| {
            if let WindowEventData::General(close_event) = &event {
                if close_event.can_veto() {
                    close_event.veto();
                    shell.borrow().frame.show(false);
                    return;
                }
            }
            event.skip(true);
        });
    }
}

/// Escape hides the window, and its beep is silenced.
///
/// Handling only KEY_DOWN is not enough: Windows still translates the key
/// press into a WM_CHAR, and every native control that cannot use Escape —
/// the edit *and* the list — answers it with a MessageBeep. Both events have
/// to be eaten, on every window that can hold the focus, which is why this is
/// bound to the frame, the panel, the edit and the list alike.
fn bind_hide_on_escape<W: WindowEvents>(shell: &SharedShell, target: &W) {
    let shell = shell.clone();
    target.on_key_down(move |event| {
        if is_escape(&event) {
            shell.borrow().frame.show(false);
            return;
        }
        event.skip(true);
    });
    target.on_char(|event| {
        if is_escape(&event) {
            return;
        }
        event.skip(true);
    });
}

fn is_escape(event: &WindowEventData) -> bool {
    match event {
        WindowEventData::Keyboard(key_event) => key_event.get_key_code() == Some(27),
        _ => false,
    }
}

/// wxWidgets key codes (wxdragon exposes none): WXK_UP / WXK_DOWN.
const WXK_UP: i32 = 315;
const WXK_DOWN: i32 = 317;

pub fn toggle_visibility(shell: &SharedShell) {
    let visible = {
        let s = shell.borrow();
        s.frame.is_shown()
    };
    if visible {
        let s = shell.borrow();
        s.frame.show(false);
        s.sounds.play("hide");
    } else {
        {
            let mut s = shell.borrow_mut();
            s.frame.show(true);
            s.sounds.play("show");
            s.frame.raise();
            // Raise only reorders the window, and within an app that is already
            // frontmost. The hotkey arrives while another app is active, so
            // without this the frame appears with no keyboard: typing still
            // goes to the app the user came from, and VoiceOver — which only
            // announces for the frontmost app — stays silent too.
            #[cfg(target_os = "macos")]
            unsafe {
                crate::macos::activate_window(s.frame.get_handle())
            };
            s.edit.set_focus();
            #[cfg(target_os = "macos")]
            unsafe {
                crate::macos::log_activation_state(s.frame.get_handle())
            };
            s.edit.change_value("");
            s.mode = if s.snippets_on_invoke() { UiMode::Snippets } else { UiMode::Commands };
        }
        update_list(shell);
    }
}

pub fn update_list(shell: &SharedShell) {
    let mut s = shell.borrow_mut();
    // Connecting, and asking for the vault's master password, need the shell
    // unborrowed, so they wait until the end.
    let mut entry = ModeEntry::Nothing;

    // Trigger characters switch modes and are consumed.
    let value = s.edit.get_value();
    if value.len() == 1 {
        if let Some(new_mode) = UiMode::from_trigger_char(value.chars().next().unwrap()) {
            entry = apply_mode_switch(&mut s, new_mode);
        }
    }

    // The sort control and the terminal button only apply to commands mode;
    // hide them elsewhere (a hidden button also gives its Alt+T mnemonic back).
    let show_commands_only = s.mode == UiMode::Commands;
    if s.sort_choice.is_shown() != show_commands_only {
        s.sort_label.show(show_commands_only);
        s.sort_choice.show(show_commands_only);
        s.terminal_button.show(show_commands_only);
        s.panel.layout();
    }

    // Same button, two jobs: arguments in commands mode, the program file in
    // apps mode. A screen reader reads the label before the press, so it has
    // to name the one that will actually happen.
    let copy_args_label = copy_args_label(s.mode);
    if s.copy_args_button.get_label() != copy_args_label {
        s.copy_args_button.set_label(&copy_args_label);
        s.panel.layout();
    }

    let value = s.edit.get_value();
    let search = value.to_lowercase();
    let mode = s.mode;
    s.items = s.controller.items_for(&search, mode);
    s.list.clear();
    for item in &s.items {
        // Stats, region and conversion lines are full sentences; don't clip
        // them so the screen reader announces the whole thing.
        let mut label: String = match item.kind {
            ItemKind::Stat | ItemKind::Region { .. } | ItemKind::Conversion { .. } => {
                item.name.clone()
            }
            _ => item.name.chars().take(40).collect(),
        };
        if !item.shortcut.is_empty() {
            label.push_str(&format!("({})", item.shortcut));
        }
        s.list.append(&label);
    }

    // In SSH mode the input field holds the command being typed, not a
    // search: jumping the selection back to the top of the transcript and
    // reading it out on every keystroke would be useless noise.
    if !s.items.is_empty() && mode != UiMode::Ssh {
        s.list.set_selection(0, true);
        if !value.is_empty() {
            let count = s.items.len();
            let first = s.list.get_string(0).unwrap_or_default();
            if count == 1 {
                speak_now(&first, true);
            } else {
                let msg = tr("{}, {} search results shown, use down arrow to access more results")
                    .replacen("{}", &first, 1)
                    .replacen("{}", &count.to_string(), 1);
                speak_now(&msg, true);
            }
        }
    }

    drop(s);
    finish_entry(shell, entry);
}

/// The first menu item id used by the modes menu; each mode takes the id
/// `MODE_MENU_BASE_ID + its index in UiMode::MENU_MODES`.
const MODE_MENU_BASE_ID: i32 = 6100;

/// Work that entering a mode leaves for the caller to do once the shell is no
/// longer borrowed, because it opens a connection or a modal dialog.
#[derive(PartialEq, Eq, Clone, Copy)]
enum ModeEntry {
    Nothing,
    Ssh,
    Vault,
}

/// Apply a mode change on an already-borrowed shell: announce it, run the
/// mode's one-time setup, select it and clear the input. Returns what the
/// caller still has to do once it has dropped the borrow (see [`finish_entry`]).
fn apply_mode_switch(s: &mut Shell, new_mode: UiMode) -> ModeEntry {
    speak_now(&mode_announcement(new_mode), true);
    let mut entry = ModeEntry::Nothing;
    match new_mode {
        UiMode::Snippets => s.controller.reload_snippets(),
        UiMode::Steam => s.controller.rescan_steam(),
        // Rescanned on every entry, like Steam: programs get installed while
        // the launcher sits in the tray, and a stale list is a mode that
        // cannot reach the app you installed this morning. The walk runs
        // while the announcement above is still being spoken.
        UiMode::Apps => s.controller.rescan_apps(),
        UiMode::Ssh => entry = ModeEntry::Ssh,
        UiMode::Vault => entry = ModeEntry::Vault,
        _ => {}
    }
    s.mode = new_mode;
    s.edit.change_value("");
    entry
}

/// Run the deferred half of a mode switch. Must be called with the shell
/// unborrowed: both of these open something modal.
fn finish_entry(shell: &SharedShell, entry: ModeEntry) {
    match entry {
        ModeEntry::Nothing => {}
        ModeEntry::Ssh => crate::ssh_flows::enter_ssh_mode(shell),
        ModeEntry::Vault => crate::vault_flows::enter_vault_mode(shell),
    }
}

/// Switch to `new_mode` from outside `update_list` (the modes menu). Mirrors
/// the trigger-character path: apply the switch, refresh the list, and finish
/// entry once the shell is unborrowed.
pub fn switch_to_mode(shell: &SharedShell, new_mode: UiMode) {
    let entry = {
        let mut s = shell.borrow_mut();
        apply_mode_switch(&mut s, new_mode)
    };
    update_list(shell);
    finish_entry(shell, entry);
}

/// What the copy-args button says it will do in `mode`. Both labels keep the
/// same mnemonic, so Alt+O works wherever the button is.
fn copy_args_label(mode: UiMode) -> String {
    match mode {
        UiMode::Apps => tr("C&opy program file (Alt+O)"),
        _ => tr("C&opy Args (Alt+O)"),
    }
}

/// The spoken sentence announced on entering a mode.
fn mode_announcement(mode: UiMode) -> String {
    match mode {
        UiMode::Snippets => tr("snippet mode"),
        UiMode::Clipboard => tr("Clipboard history mode"),
        UiMode::Commands => tr("commands mode"),
        UiMode::Steam => tr("Steam games mode"),
        UiMode::Apps => tr("applications mode, type the name of a program to launch it"),
        UiMode::Screenshots => tr("screenshots mode"),
        UiMode::Timers => tr("timers mode"),
        UiMode::Alarms => tr("alarms mode"),
        UiMode::Notebrook => tr("Notebrook new note mode, type your note and press enter"),
        UiMode::Realtime => tr("realtime data mode"),
        UiMode::Stats => tr("statistics mode"),
        UiMode::Ssh => tr("SSH mode, type a command and press enter"),
        UiMode::Emoji => tr("emoji mode, type a description and press enter to copy"),
        UiMode::Units => tr("unit conversion mode, type a number and choose a conversion"),
        UiMode::Vault => tr("encrypted vault mode"),
        UiMode::Regions => unreachable!("no trigger char"),
    }
}

/// The short, human name of a mode, used as the modes-menu item label.
fn mode_name(mode: UiMode) -> String {
    match mode {
        UiMode::Commands => tr("Commands"),
        UiMode::Snippets => tr("Snippets"),
        UiMode::Clipboard => tr("Clipboard history"),
        UiMode::Steam => tr("Steam games"),
        UiMode::Apps => tr("Applications"),
        UiMode::Screenshots => tr("Screenshots"),
        UiMode::Timers => tr("Timers"),
        UiMode::Alarms => tr("Alarms"),
        UiMode::Notebrook => tr("Notebrook"),
        UiMode::Realtime => tr("Realtime data"),
        UiMode::Stats => tr("Statistics"),
        UiMode::Ssh => tr("SSH"),
        UiMode::Emoji => tr("Emoji"),
        UiMode::Units => tr("Unit conversion"),
        UiMode::Vault => tr("Encrypted vault"),
        UiMode::Regions => tr("Regions"),
    }
}

/// Pop up the accessible modes menu: every mode with its trigger character,
/// with the current one checked. Selecting an item routes through the frame's
/// menu handler (see `bind_events`) into [`switch_to_mode`].
fn show_modes_menu(shell: &SharedShell) {
    let (frame, current) = {
        let s = shell.borrow();
        (s.frame, s.mode)
    };
    let mut builder = Menu::builder();
    for (index, &mode) in UiMode::MENU_MODES.iter().enumerate() {
        let trigger = mode.trigger_char().unwrap_or(' ');
        let label = format!("{} ({})", mode_name(mode), trigger);
        builder = builder.append_check_item(MODE_MENU_BASE_ID + index as i32, &label, "");
    }
    let mut menu = builder.build();
    if let Some(index) = UiMode::MENU_MODES.iter().position(|&m| m == current) {
        menu.check_item(MODE_MENU_BASE_ID + index as i32, true);
    }
    frame.popup_menu(&mut menu, None);
}

pub fn run_clicked(shell: &SharedShell) {
    let mode = shell.borrow().mode;
    if mode == UiMode::Notebrook {
        send_notebrook_note(shell);
        return;
    }
    if mode == UiMode::Ssh {
        crate::ssh_flows::run_ssh_command(shell);
        return;
    }

    let item = {
        let s = shell.borrow();
        let Some(index) = s.list.get_selection() else { return };
        let Some(item) = s.items.get(index as usize).cloned() else { return };
        item
    };

    match item.kind.clone() {
        // Timers and alarms are toggled in place; keep the window open.
        ItemKind::Timer => {
            {
                let s = shell.borrow_mut();
                let now = s.controller.clock.now();
                let enabled = s.controller.timers.toggle(&item.id, now);
                s.sounds.play("match");
                let state = if enabled == Some(true) { tr("started") } else { tr("stopped") };
                speak_now(&format_args(&tr("Timer {state}"), &[("state", Arg::Str(&state))]), true);
            }
            update_list(shell);
        }
        ItemKind::Alarm => {
            {
                let s = shell.borrow_mut();
                let enabled = s.controller.alarms.toggle(&item.id);
                s.sounds.play("match");
                let state = if enabled == Some(true) { tr("on") } else { tr("off") };
                speak_now(&format_args(&tr("Alarm {state}"), &[("state", Arg::Str(&state))]), true);
            }
            update_list(shell);
        }
        // Realtime lookups fetch in the background and announce the value
        // when it arrives; the window stays open so the user can query
        // several values in a row.
        ItemKind::Realtime { key } => {
            let sounds = shell.borrow().sounds.clone();
            sounds.play("run");
            speak_now(
                &format_args(&tr("Fetching {name}"), &[("name", Arg::Str(&item.name))]),
                true,
            );
            let name = item.name.clone();
            std::thread::spawn(move || {
                let result = launchtype_services::realtime::fetch_value(&key);
                wxdragon::call_after(Box::new(move || match result {
                    Ok(announcement) => {
                        sounds.play("match");
                        speak_now(&announcement, true);
                    }
                    Err(error) => speak_now(
                        &format_args(
                            &tr("Could not fetch {name}: {reason}"),
                            &[("name", Arg::Str(&name)), ("reason", Arg::Str(&error.message()))],
                        ),
                        true,
                    ),
                }));
            });
        }
        // Stats lines are informational; re-speak on enter, keep the window.
        ItemKind::Stat => speak_now(&item.name, true),
        // A conversion copies the number on its own — the sentence around it
        // has already been read out — and keeps the window open, because one
        // conversion is usually followed by another.
        ItemKind::Conversion { result } => match result {
            Some(result) => {
                let s = shell.borrow();
                clipboard::set_text(&result);
                s.sounds.play("copy");
                speak_now(
                    &format_args(&tr("{value} copied"), &[("value", Arg::Str(&result))]),
                    true,
                );
            }
            None => speak_now(&tr("Type a number to convert first"), true),
        },
        // The vault decrypts and copies (and hides the window) itself, because
        // it has clipboard hygiene to arrange around the copy; its action rows
        // open dialogs and so must keep the window up.
        ItemKind::VaultEntry => crate::vault_flows::copy_secret(shell, &item.id, &item.name),
        ItemKind::VaultAction { action } => crate::vault_flows::run_action(shell, action),
        // A region: crop it out of the last screenshot, copy the crop, and
        // describe it. Keep the window open so more regions can be chosen.
        ItemKind::Region { r#box } => crate::ai_flows::crop_and_describe_region(shell, r#box),
        // Screenshot actions hide the window themselves rather than being
        // hidden by the arm below: "grab specific region" asks what to crop
        // first, and a modal parented to an already-hidden frame never comes
        // to the foreground. They report their own failures too, because they
        // finish long after this call returns.
        ItemKind::Screenshot { action } => {
            crate::ai_flows::handle_screenshot_action(shell, action)
        }
        other => {
            shell.borrow().frame.show(false);
            let result = run_hidden_action(shell, &item, other);
            if let Err(message) = result {
                // Python interpolated this inside _() as an f-string, so the
                // msgid never existed in the catalog: always English there too.
                let msg = format!("Something went wrong while running your command: {message}");
                show_error(&shell.borrow().frame, "Oops...", &msg);
            }
        }
    }
}

fn run_hidden_action(shell: &SharedShell, item: &Item, kind: ItemKind) -> Result<(), String> {
    match kind {
        ItemKind::Command { path, args, run_as_admin } => {
            let (result, id) = {
                let s = shell.borrow();
                (
                    run_command(
                        &path,
                        &args,
                        run_as_admin,
                        &s.sounds,
                        launchtype_services::portable::vars(),
                    ),
                    item.id.clone(),
                )
            };
            result.map_err(|e| e.to_string())?;
            shell.borrow_mut().controller.commands.record_run(&id);
        }
        ItemKind::Snippet => {
            clipboard::set_text(&item.name);
            shell.borrow().sounds.play("copy");
        }
        // The name is what the list showed; the glyph rides along in the kind.
        ItemKind::Emoji { emoji } => {
            clipboard::set_text(emoji);
            shell.borrow().sounds.play("copy");
        }
        ItemKind::Clip => {
            let s = shell.borrow();
            let mut history = s.controller.clipboard.lock().unwrap();
            history.delete_by_text(&item.name);
            history.forget_last_value();
            drop(history);
            s.sounds.play("copy");
            clipboard::set_text(&item.name);
        }
        ItemKind::Steam { appid } => {
            open::that_detached(steam::rungameid_url(&appid)).map_err(|e| e.to_string())?;
            shell.borrow().sounds.play("run");
        }
        ItemKind::App { target } => {
            apps::launch(&target).map_err(|e| e.to_string())?;
            shell.borrow().sounds.play("run");
        }
        _ => {}
    }
    Ok(())
}

/// Open a terminal window at the folder the focused command's arguments point
/// at: a folder argument opens itself, a file argument opens its containing
/// folder. The command's `path` (the executable) is deliberately ignored —
/// the folder worth a shell is the one the command acts on, not the one the
/// program is installed in.
fn open_in_terminal(shell: &SharedShell) {
    let folder = {
        let s = shell.borrow();
        let Some(index) = s.list.get_selection() else { return };
        let Some(item) = s.items.get(index as usize) else { return };
        let ItemKind::Command { args, .. } = &item.kind else { return };
        let vars = launchtype_services::portable::vars();
        launchtype_core::portable::arg_segments(args)
            .iter()
            .map(|segment| launchtype_core::portable::expand(segment, vars))
            .find_map(|expanded| {
                let path = std::path::Path::new(&expanded);
                if path.is_dir() {
                    Some(path.to_path_buf())
                } else if path.is_file() {
                    path.parent().map(|parent| parent.to_path_buf())
                } else {
                    None
                }
            })
    };
    let Some(folder) = folder else {
        speak_now(&tr("No folder in this command's arguments"), true);
        return;
    };
    match launchtype_services::runner::open_terminal_at(&folder) {
        Ok(()) => {
            let s = shell.borrow();
            s.sounds.play("run");
            s.frame.show(false);
        }
        Err(error) => {
            let frame = shell.borrow().frame;
            show_error(&frame, &tr("Cannot open terminal"), &error.to_string());
        }
    }
}

fn send_notebrook_note(shell: &SharedShell) {
    let (note, mut url, mut token, frame) = {
        let s = shell.borrow();
        (
            s.edit.get_value().trim().to_string(),
            s.settings.settings.notebrook_url.clone(),
            s.settings.settings.notebrook_token.clone(),
            s.frame,
        )
    };
    if note.is_empty() {
        speak_now(&tr("No note entered"), true);
        return;
    }
    if url.is_empty() || token.is_empty() {
        // Prompt once and persist; cancelling aborts the send.
        let Some((new_url, new_token)) =
            crate::dialogs::notebrook_credentials_dialog(&frame, &url, &token)
        else {
            return;
        };
        url = new_url;
        token = new_token;
        let mut s = shell.borrow_mut();
        s.settings.settings.notebrook_url = url.clone();
        s.settings.settings.notebrook_token = token.clone();
        let _ = s.settings.save();
    }
    match notebrook::send_note(&url, &token, NOTEBROOK_CHANNEL, &note) {
        Ok(()) => {
            let mut s = shell.borrow_mut();
            s.sounds.play("run");
            speak_now(&tr("Note sent to {}").replacen("{}", NOTEBROOK_CHANNEL, 1), true);
            s.mode = UiMode::Commands;
            s.edit.change_value("");
            s.frame.show(false);
        }
        Err(e) => {
            if e.unauthorized {
                // Forget the rejected credentials so we ask again next time.
                let mut s = shell.borrow_mut();
                s.settings.settings.notebrook_url.clear();
                s.settings.settings.notebrook_token.clear();
                let _ = s.settings.save();
            }
            speak_now(&tr("Note not sent"), true);
            show_error(&shell.borrow().frame, &tr("Note not sent"), &e.message);
        }
    }
}

/// Merge another commands file into this one.
///
/// Additive from end to end: pick a file, work out what in it is genuinely new
/// (see [`launchtype_core::merge`]), let the user tick what to keep, and append
/// that. Commands already here are never listed, never changed and never
/// removed, so there is no replace question to answer anywhere in the flow.
fn merge_clicked(shell: &SharedShell) {
    let frame = shell.borrow().frame;
    let file_dialog = FileDialog::builder(&frame)
        .with_message(&tr("Choose a commands file to merge in"))
        .with_default_dir(&std::env::current_dir().unwrap_or_default().to_string_lossy())
        .with_wildcard(&tr("Command files (*.json)|*.json|All files (*.*)|*.*"))
        .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
        .build();
    if file_dialog.show_modal() != wxdragon::id::ID_OK {
        return;
    }
    let Some(path) = file_dialog.get_path() else { return };

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            show_error(&frame, &tr("Cannot merge"), &error.to_string());
            return;
        }
    };
    let incoming = match launchtype_core::merge::parse(&text) {
        Ok(file) => file,
        Err(_) => {
            // Both failures land the user in the same place — pick another
            // file — so they read as one message rather than a diagnosis.
            show_error(
                &frame,
                &tr("Cannot merge"),
                &tr("That file is not a Launchtype commands file."),
            );
            return;
        }
    };

    let plan = {
        let s = shell.borrow();
        launchtype_core::merge::plan(
            &s.controller.commands.file,
            &incoming,
            launchtype_services::portable::vars(),
            &|path| std::path::Path::new(path).exists(),
        )
    };
    if plan.candidates.is_empty() {
        // An empty commands.json is easy to end up with (the Settings dropdown
        // creates one from a typed name), and "already in your list" would be
        // a puzzling thing to say about a file holding nothing.
        let reason = if incoming.commands.is_empty() {
            tr("That file has no commands in it.")
        } else {
            tr("Every command in that file is already in your list.")
        };
        show_alert(&frame, &tr("Nothing to merge"), &reason);
        return;
    }

    let file_name = std::path::Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or(path);
    let selected = crate::dialogs::merge_dialog(&frame, &file_name, &plan);
    if selected.is_empty() {
        return;
    }

    let added = shell.borrow_mut().controller.commands.merge_commands(&selected);
    update_list(shell);
    shell.borrow().sounds.play("match");
    speak_now(
        &format_args(&tr("{count} commands merged"), &[("count", Arg::Int(added as i64))]),
        true,
    );
}

pub fn exit_app(shell: &SharedShell) {
    let mut s = shell.borrow_mut();
    if let Some(mut poller) = s.poller.take() {
        poller.stop();
    }
    if let Some(mut scheduler) = s.scheduler.take() {
        scheduler.stop();
    }
    if let Some(mut locker) = s.vault_locker.take() {
        locker.stop();
    }
    s.frame.close(true);
}

/// Put a dialog up against the main window, making sure that window is on
/// screen first.
///
/// A modal parented to a frame that is hidden does not come to the foreground
/// on Windows — the app just looks like it ignored you. That happens both at
/// startup with `start_minimized` (or `-m`), where the frame is created but
/// never shown, and after any action that hides the window while it works (the
/// screenshot flows). Show the frame for as long as the dialog is up, then put
/// it back the way it was.
///
/// Deliberately quiet: no "show" sound and no focus move, because this is the
/// app interrupting the user, not the user invoking the app.
fn with_window_up<T>(shell: &SharedShell, ask: impl FnOnce(&Frame) -> T) -> T {
    let (frame, was_hidden) = {
        let s = shell.borrow();
        (s.frame, !s.frame.is_shown())
    };
    // Showing the frame is what gives the process a foreground window at all;
    // without one Windows will not let the dialog take the foreground either.
    if was_hidden {
        frame.show(true);
        frame.raise();
    }
    let answer = ask(&frame);
    if was_hidden {
        frame.show(false);
    }
    answer
}

/// Report a failure, surfacing the main window first so the message can take
/// the foreground even when that window is down — the app started minimized,
/// or an action hid it to get out of the way (the screenshot flows).
pub fn report_error(shell: &SharedShell, title: &str, text: &str) {
    with_window_up(shell, |frame| show_error(frame, title, text));
}

/// On startup, warn once when a keyboard shortcut is shared by two or more
/// commands. Only the first matching command ever runs, so the rest are dead;
/// listing them lets the user go fix the clash.
pub fn warn_shortcut_conflicts(shell: &SharedShell) {
    let conflicts = shell.borrow().controller.commands.file.shortcut_conflicts();
    if conflicts.is_empty() {
        return;
    }
    let mut body = tr(
        "The following keyboard shortcuts are each assigned to more than one command. Only the first matching command will run; please edit them so every shortcut is unique.",
    );
    body.push_str("\n\n");
    for (shortcut, names) in &conflicts {
        body.push_str(&format!("{}: {}\n", shortcut, names.join(", ")));
    }
    with_window_up(shell, |frame| {
        show_alert(frame, &tr("Shortcut conflicts"), body.trim_end())
    });
}

/// On startup, offer to replace machine-specific paths (a hardcoded user
/// folder, a browser executable) with placeholders that each machine resolves
/// for itself.
///
/// Runs after the shortcut-conflict warning so the two never stack up, and is
/// skipped once the user answers "never ask again". The scan is pure string
/// matching against this machine's placeholder values — it must never touch
/// the filesystem, because it runs before the event loop starts and one probe
/// of an offline network share once held startup hostage for seconds.
pub fn check_portability(shell: &SharedShell) {
    let report = {
        let s = shell.borrow();
        if !s.settings.settings.portability_check {
            return;
        }
        launchtype_core::portable::scan(
            &s.controller.commands.file,
            &s.settings.settings.machine_specific_paths(),
            launchtype_services::portable::vars(),
        )
    };
    if report.fixes.is_empty() {
        return;
    }

    let choice =
        with_window_up(shell, |frame| crate::dialogs::portability_dialog(frame, &report));
    match choice {
        crate::dialogs::PortabilityChoice::NotNow => {}
        crate::dialogs::PortabilityChoice::NeverAsk => {
            let mut s = shell.borrow_mut();
            s.settings.settings.portability_check = false;
            let _ = s.settings.save();
        }
        crate::dialogs::PortabilityChoice::Fix(selected) => {
            if selected.is_empty() {
                return;
            }
            let changed = {
                let mut s = shell.borrow_mut();
                let vars = launchtype_services::portable::vars();
                let mut changed = launchtype_core::portable::apply(
                    &mut s.controller.commands.file,
                    &selected,
                    vars,
                );
                s.controller.commands.sync();
                // Settings hold machine-specific paths too, and they were
                // counted in the same report. Borrow the struct once so the
                // two fields are disjoint borrows rather than two of `s`.
                let settings = &mut s.settings.settings;
                for field in [&mut settings.steam_library, &mut settings.ssh_key_path] {
                    if let Some(fixed) =
                        launchtype_core::portable::apply_to_setting(field, &selected, vars)
                    {
                        *field = fixed;
                        changed += 1;
                    }
                }
                let _ = s.settings.save();
                changed
            };
            // The rows cache each command's path; rebuild so a run right after
            // the migration uses the rewritten one.
            update_list(shell);
            speak_now(
                &format_args(
                    &tr("{count} paths made portable"),
                    &[("count", Arg::Int(changed as i64))],
                ),
                true,
            );
        }
    }
}

pub fn show_alert(parent: &Frame, title: &str, text: &str) {
    let dialog = wxdragon::dialogs::message_dialog::MessageDialog::builder(parent, text, title)
        .with_style(wxdragon::dialogs::message_dialog::MessageDialogStyle::OK)
        .build();
    dialog.show_modal();
}

pub fn show_error(parent: &Frame, title: &str, text: &str) {
    let dialog = wxdragon::dialogs::message_dialog::MessageDialog::builder(parent, text, title)
        .with_style(
            wxdragon::dialogs::message_dialog::MessageDialogStyle::OK
                | wxdragon::dialogs::message_dialog::MessageDialogStyle::IconError,
        )
        .build();
    dialog.show_modal();
}
