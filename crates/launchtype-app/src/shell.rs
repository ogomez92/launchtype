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
use launchtype_services::{clipboard, notebrook, steam};
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
    // name ("text") instead of this label, so name it explicitly (as the results
    // ListBox below already does).
    edit.set_name(&tr("Input Field"));
    edit_sizer.add(&edit_label, 0, SizerFlag::All, 0);
    edit_sizer.add(&edit, 0, SizerFlag::All, 0);
    sizer.add_sizer(&edit_sizer, 0, SizerFlag::All, 0);

    // Give the results list its own label so screen readers don't fall back
    // to the nearest preceding control (e.g. the input field's label).
    let results_label = StaticText::builder(&panel).with_label(&tr("Results")).build();
    sizer.add(&results_label, 0, SizerFlag::All, 0);
    let list = ListBox::builder(&panel).build();
    list.set_name(&tr("Results"));
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
    let copy_args_button = Button::builder(&panel).with_label(&tr("C&opy Args (Alt+O)")).build();
    let snippets_button = Button::builder(&panel).with_label(&tr("Open &Snippets folder")).build();
    let new_snippet_button = Button::builder(&panel).with_label(&tr("&New snipet")).build();
    let run_button = Button::builder(&panel).with_label(&tr("&Run")).build();
    let help_button = Button::builder(&panel).with_label(&tr("&Help")).build();
    let settings_button = Button::builder(&panel).with_label(&tr("Se&ttings...")).build();
    let exit_button = Button::builder(&panel).with_label(&tr("E&xit")).build();
    for b in [
        &add_button, &edit_button, &copy_button, &delete_button, &copy_args_button,
        &snippets_button, &new_snippet_button, &run_button, &help_button,
        &settings_button, &exit_button,
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
        screenshot_image: None,
        screenshot_sent_size: None,
    }));

    bind_events(
        &shell,
        [
            add_button, edit_button, copy_button, delete_button, copy_args_button,
            snippets_button, new_snippet_button, run_button, help_button,
            settings_button, exit_button,
        ],
    );
    shell
}

fn bind_events(shell: &SharedShell, buttons: [Button; 11]) {
    let [add_button, edit_button, copy_button, delete_button, copy_args_button, snippets_button, new_snippet_button, run_button, help_button, settings_button, exit_button] =
        buttons;
    let (frame, edit, list, panel, sort_choice) = {
        let s = shell.borrow();
        (s.frame, s.edit, s.list, s.panel, s.sort_choice)
    };

    {
        let shell = shell.clone();
        edit.on_text_changed(move |_| update_list(&shell));
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
            {
                let mut s = shell.borrow_mut();
                let frame = s.frame;
                match s.mode {
                    UiMode::Timers => {
                        crate::dialogs::add_timer_dialog(&frame, &mut s.controller);
                    }
                    UiMode::Alarms => {
                        crate::dialogs::add_alarm_dialog(&frame, &mut s.controller);
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
            {
                let mut s = shell.borrow_mut();
                let Some(index) = s.list.get_selection() else { return };
                let Some(item) = s.items.get(index as usize).cloned() else { return };
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
            {
                let mut s = shell.borrow_mut();
                let Some(index) = s.list.get_selection() else { return };
                let Some(item) = s.items.get(index as usize).cloned() else { return };
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
            if let ItemKind::Command { args, .. } = &item.kind {
                if !args.is_empty() {
                    clipboard::set_text(args);
                    s.sounds.play("copy");
                    speak_now(&tr("Arguments copied"), true);
                    return;
                }
            }
            speak_now(&tr("No arguments"), true);
        });
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
        exit_button.on_click(move |_| exit_app(&shell));
    }

    // Escape hides the window instead of closing the app.
    bind_hide_on_escape(shell, &frame);
    bind_hide_on_escape(shell, &panel);
    bind_hide_on_escape(shell, &edit);
    bind_hide_on_escape(shell, &list);

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
            s.edit.set_focus();
            s.edit.change_value("");
            s.mode = if s.snippets_on_invoke() { UiMode::Snippets } else { UiMode::Commands };
        }
        update_list(shell);
    }
}

pub fn update_list(shell: &SharedShell) {
    let mut s = shell.borrow_mut();
    // Connecting needs the shell unborrowed, so it waits until the end.
    let mut entered_ssh = false;

    // Trigger characters switch modes and are consumed.
    let value = s.edit.get_value();
    if value.len() == 1 {
        if let Some(new_mode) = UiMode::from_trigger_char(value.chars().next().unwrap()) {
            let announcement = match new_mode {
                UiMode::Snippets => tr("snippet mode"),
                UiMode::Clipboard => tr("Clipboard history mode"),
                UiMode::Commands => tr("commands mode"),
                UiMode::Steam => tr("Steam games mode"),
                UiMode::Screenshots => tr("screenshots mode"),
                UiMode::Timers => tr("timers mode"),
                UiMode::Alarms => tr("alarms mode"),
                UiMode::Notebrook => tr("Notebrook new note mode, type your note and press enter"),
                UiMode::Realtime => tr("realtime data mode"),
                UiMode::Stats => tr("statistics mode"),
                UiMode::Ssh => tr("SSH mode, type a command and press enter"),
                UiMode::Regions => unreachable!("no trigger char"),
            };
            speak_now(&announcement, true);
            match new_mode {
                UiMode::Snippets => s.controller.reload_snippets(),
                UiMode::Steam => s.controller.rescan_steam(),
                UiMode::Ssh => entered_ssh = true,
                _ => {}
            }
            s.mode = new_mode;
            s.edit.change_value("");
        }
    }

    // The sort control only applies to commands mode; hide it elsewhere.
    let show_sort = s.mode == UiMode::Commands;
    if s.sort_choice.is_shown() != show_sort {
        s.sort_label.show(show_sort);
        s.sort_choice.show(show_sort);
        s.panel.layout();
    }

    let value = s.edit.get_value();
    let search = value.to_lowercase();
    let mode = s.mode;
    s.items = s.controller.items_for(&search, mode);
    s.list.clear();
    for item in &s.items {
        // Stats and region lines are full sentences; don't clip them so the
        // screen reader announces the whole thing.
        let mut label: String = match item.kind {
            ItemKind::Stat | ItemKind::Region { .. } => item.name.clone(),
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
                let msg = tr("{}, {} search results shown, use tab and down arrow to access more results")
                    .replacen("{}", &first, 1)
                    .replacen("{}", &count.to_string(), 1);
                speak_now(&msg, true);
            }
        }
    }

    drop(s);
    if entered_ssh {
        crate::ssh_flows::enter_ssh_mode(shell);
    }
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
        // A region: crop it out of the last screenshot, copy the crop, and
        // describe it. Keep the window open so more regions can be chosen.
        ItemKind::Region { r#box } => crate::ai_flows::crop_and_describe_region(shell, r#box),
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
                (run_command(&path, &args, run_as_admin, &s.sounds), item.id.clone())
            };
            result.map_err(|e| e.to_string())?;
            shell.borrow_mut().controller.commands.record_run(&id);
        }
        ItemKind::Snippet => {
            clipboard::set_text(&item.name);
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
        ItemKind::Screenshot { action } => {
            crate::ai_flows::handle_screenshot_action(shell, action)?;
        }
        _ => {}
    }
    Ok(())
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

pub fn exit_app(shell: &SharedShell) {
    let mut s = shell.borrow_mut();
    if let Some(mut poller) = s.poller.take() {
        poller.stop();
    }
    if let Some(mut scheduler) = s.scheduler.take() {
        scheduler.stop();
    }
    s.frame.close(true);
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
