//! Entry point — port of `src/main.py`: i18n, CLI flags, settings,
//! data stores, speech, UI, global hotkey, main loop.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod controller;
mod dialogs;
mod hotkey;
mod shell;
mod speech;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use launchtype_core::clock::SystemClock;
use launchtype_core::i18n::tr;
use launchtype_core::settings::SettingsStore;
use launchtype_services::alerts::fire_alert;
use launchtype_services::poller::ClipboardPoller;
use launchtype_services::scheduler::Scheduler;
use launchtype_services::sounds::SoundPlayer;
use launchtype_services::stores::{AlarmStore, CommandsStore, TimerStore};

#[derive(Default)]
struct CliArgs {
    start_minimized: bool,
    snippets_on_invoke: bool,
    quiet: bool,
    commands_file: Option<String>,
    steam_library: Option<String>,
}

fn parse_cli() -> CliArgs {
    let mut args = CliArgs::default();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-m" | "--start-minimized" => args.start_minimized = true,
            "-s" | "--snippets-on-invoke" => args.snippets_on_invoke = true,
            "-q" | "--quiet" => args.quiet = true,
            "-c" | "--commands" => args.commands_file = iter.next(),
            "-l" | "--steam-library" => args.steam_library = iter.next(),
            other => log::warn!("unknown argument {other:?}"),
        }
    }
    args
}

/// Portable data location: the directory containing the executable (next to
/// the .app bundle on macOS). Falls back to the current directory.
fn data_dir() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    #[cfg(target_os = "macos")]
    let exe_dir = exe_dir.map(|dir| {
        // .../Launchtype.app/Contents/MacOS -> the folder containing the bundle
        match dir.ancestors().nth(3) {
            Some(outside) if dir.ends_with("Contents/MacOS") => outside.to_path_buf(),
            _ => dir,
        }
    });
    exe_dir.unwrap_or_else(|| PathBuf::from("."))
}

/// Locate a bundled asset dir (locale/, sounds/): next to the exe first,
/// then the working directory (useful for `cargo run` during development).
fn asset_dir(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(name);
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from(name)
}

fn init_i18n() {
    let lang = sys_locale::get_locale().unwrap_or_default();
    let code: String = lang.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    if code.is_empty() || code == "en" {
        return;
    }
    let mo_path = asset_dir("locale").join(&code).join("LC_MESSAGES").join("launchtype.mo");
    match std::fs::File::open(&mo_path) {
        Ok(file) => match gettext::Catalog::parse(file) {
            Ok(catalog) => launchtype_core::i18n::set_catalog(Some(catalog)),
            Err(e) => log::warn!("failed to parse {}: {e}", mo_path.display()),
        },
        Err(_) => log::info!("no catalog for {code:?}, using English"),
    }
}

fn main() {
    env_logger::init();
    let cli = parse_cli();

    // All data files live next to the app; making it the working directory
    // keeps every relative path (snippets/, screenshots/) Python-compatible.
    let _ = std::env::set_current_dir(data_dir());

    init_i18n();

    let settings = SettingsStore::load("settings.json");
    let effective_start_minimized = cli.start_minimized || settings.settings.start_minimized;
    let effective_sounds = settings.settings.enable_sounds && !cli.quiet;
    let steam_library = cli
        .steam_library
        .clone()
        .unwrap_or_else(|| settings.settings.steam_library.clone());
    let commands_file = cli.commands_file.clone().unwrap_or_else(|| "commands.json".into());

    let sounds = Arc::new(SoundPlayer::new(asset_dir("sounds"), effective_sounds));

    let now = chrono::Local::now();
    let commands = CommandsStore::load(commands_file);
    let timers = TimerStore::load("timers.json", now);
    let alarms = AlarmStore::load("alarms.json");
    let clipboard = Arc::new(Mutex::new(launchtype_core::clipboard_history::load_history(
        std::path::Path::new("clipboard_history.json"),
    )));

    let mut controller = controller::ModeController::new(
        commands,
        settings.settings.command_sort_by_uses,
        clipboard.clone(),
        timers,
        alarms,
        PathBuf::from(steam_library),
        sounds.clone(),
    );
    controller.reload_snippets();

    let cli_snippets = cli.snippets_on_invoke;
    let cli_quiet = cli.quiet;
    let sounds_for_ui = sounds.clone();

    let main_result = wxdragon::main(move |_| {
        speech::init_speech();

        let shell =
            shell::build_shell(controller, settings, sounds_for_ui.clone(), cli_snippets, cli_quiet);

        // Background services: clipboard history + timer/alarm firing.
        {
            let mut s = shell.borrow_mut();
            s.poller = Some(ClipboardPoller::start(
                clipboard.clone(),
                PathBuf::from("clipboard_history.json"),
            ));
            let speaker = speech::shared_speaker();
            let alert_sounds = sounds_for_ui.clone();
            s.scheduler = Some(Scheduler::start(
                s.controller.timers.engine.clone(),
                s.controller.alarms.engine.clone(),
                Arc::new(SystemClock),
                move |item| fire_alert(&item, &speaker, &alert_sounds),
            ));
        }

        if !effective_start_minimized {
            shell::toggle_visibility(&shell);
        }

        let frame = shell.borrow().frame;
        let shell_for_hotkey = shell.clone();
        // The guard makes the hotkey a no-op while a modal dialog holds the
        // shell borrow (matching "hotkey does nothing useful during modals").
        let on_hotkey = move || {
            if shell_for_hotkey.try_borrow_mut().is_ok() {
                shell::toggle_visibility(&shell_for_hotkey);
            }
        };
        match hotkey::register(&frame, on_hotkey) {
            Ok(hotkey) => {
                // Keep the manager + polling timer alive for the app lifetime.
                std::mem::forget(hotkey);
            }
            Err(e) => shell::show_error(
                &frame,
                "error",
                &format!("{}{e}", tr("There was an error registering the hotkey for the program: ")),
            ),
        }

        sounds_for_ui.play("logo");
    });
    if let Err(e) = main_result {
        eprintln!("launchtype failed to start: {e}");
    }
}
