//! ModeController — the Rust counterpart of `DataManager`'s per-mode item
//! dispatch: owns the data stores and answers "what does the list show for
//! this mode + search text", including the match/type sound cues.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use launchtype_core::alarms::AlarmEngine;
use launchtype_core::clipboard_history::ClipboardHistory;
use launchtype_core::clock::{Clock, SystemClock};
use launchtype_core::i18n::tr;
use launchtype_core::mode::UiMode;
use launchtype_core::search::{exact_shortcut_match, fuzzy_search};
use launchtype_core::stats::stats_labels;
use launchtype_services::snippets::{load_snippets, Snippet};
use launchtype_services::sounds::SoundPlayer;
use launchtype_services::steam::scan_games;
use launchtype_services::stores::{AlarmStore, CommandsStore, TimerStore};

/// One row of the results list, carrying everything Run needs.
#[derive(Debug, Clone)]
pub struct Item {
    /// Display + search text (snippet items carry their contents here).
    pub name: String,
    pub shortcut: String,
    pub id: String,
    pub kind: ItemKind,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Region's box is consumed by the M8 crop flow.
pub enum ItemKind {
    Command { path: String, args: String, run_as_admin: bool },
    Snippet,
    Clip,
    Steam { appid: String },
    Screenshot { action: &'static str },
    Timer,
    Alarm,
    Realtime { key: String },
    Stat,
    Region { r#box: [f64; 4] },
    /// One line of SSH command output (or of the echoed command line).
    SshOutput,
}

pub struct ModeController {
    pub commands: CommandsStore,
    pub sort_by_uses: bool,
    pub snippets: Vec<Snippet>,
    pub clipboard: Arc<Mutex<ClipboardHistory>>,
    pub timers: TimerStore,
    pub alarms: AlarmStore,
    pub steam_library: PathBuf,
    steam_games: Vec<launchtype_core::steam::SteamGame>,
    pub sounds: Arc<SoundPlayer>,
    pub clock: Arc<dyn Clock>,
    /// Transient "explore regions" state: AI-space size + labeled boxes of
    /// the last capture (the full-res image itself lives in the shell).
    pub regions: Vec<(String, [f64; 4])>,
    /// SSH mode transcript: every echoed command and output line so far.
    pub ssh_output: Vec<String>,
}

impl ModeController {
    pub fn new(
        commands: CommandsStore,
        sort_by_uses: bool,
        clipboard: Arc<Mutex<ClipboardHistory>>,
        timers: TimerStore,
        alarms: AlarmStore,
        steam_library: PathBuf,
        sounds: Arc<SoundPlayer>,
    ) -> Self {
        ModeController {
            commands,
            sort_by_uses,
            snippets: Vec::new(),
            clipboard,
            timers,
            alarms,
            steam_library,
            steam_games: Vec::new(),
            sounds,
            clock: Arc::new(SystemClock),
            regions: Vec::new(),
            ssh_output: Vec::new(),
        }
    }

    pub fn reload_snippets(&mut self) {
        self.snippets = load_snippets(std::path::Path::new("."));
    }

    pub fn rescan_steam(&mut self) {
        self.steam_games = scan_games(&self.steam_library);
    }

    pub fn items_for(&mut self, search: &str, mode: UiMode) -> Vec<Item> {
        match mode {
            UiMode::Commands => self.command_items(search),
            UiMode::Snippets => self.snippet_items(search),
            UiMode::Clipboard => self.clipboard_items(search),
            UiMode::Steam => self.steam_items(search),
            UiMode::Screenshots => self.screenshot_items(search),
            UiMode::Timers => self.timer_items(search),
            UiMode::Alarms => self.alarm_items(search),
            // The note content is taken straight from the edit field on run.
            UiMode::Notebrook => Vec::new(),
            UiMode::Realtime => self.realtime_items(search),
            UiMode::Stats => self.stats_items(),
            // The input field holds the command being typed, so it must not
            // filter the transcript away (same reasoning as screenshots mode).
            UiMode::Ssh => self.ssh_items(),
            UiMode::Regions => self.region_items(search),
        }
    }

    /// Exact-shortcut-match short-circuit (with "match" sound) then fuzzy
    /// search (with "type" sound), shared by most modes.
    fn shortcut_then_fuzzy(&self, search: &str, items: Vec<Item>, fuzzy_on_name: bool) -> Vec<Item> {
        if search.is_empty() {
            return items;
        }
        if let Some(index) = exact_shortcut_match(search, &items, |i| i.shortcut.clone()) {
            self.sounds.play("match");
            return vec![items[index].clone()];
        }
        let results = if fuzzy_on_name {
            fuzzy_search(search, items, |i| i.name.clone())
        } else {
            items
        };
        self.sounds.play("type");
        results
    }

    fn command_items(&self, search: &str) -> Vec<Item> {
        let items: Vec<Item> = self
            .commands
            .display_order(self.sort_by_uses)
            .into_iter()
            .map(|c| Item {
                name: c.name.clone(),
                shortcut: c.shortcut().to_string(),
                id: c.id.clone(),
                kind: ItemKind::Command {
                    path: c.path.clone(),
                    args: c.args().to_string(),
                    run_as_admin: c.run_as_admin(),
                },
            })
            .collect();
        self.shortcut_then_fuzzy(search, items, true)
    }

    fn snippet_items(&self, search: &str) -> Vec<Item> {
        let items: Vec<Item> = self
            .snippets
            .iter()
            .map(|s| Item {
                name: s.contents.clone(),
                shortcut: s.shortcut.clone(),
                id: String::new(),
                kind: ItemKind::Snippet,
            })
            .collect();
        if search.is_empty() {
            return items;
        }
        if let Some(index) = exact_shortcut_match(search, &items, |i| i.shortcut.clone()) {
            self.sounds.play("match");
            return vec![items[index].clone()];
        }
        // Fuzzy over "shortcut contents", like the Python snippet search.
        let results = fuzzy_search(search, items, |i| format!("{} {}", i.shortcut, i.name));
        self.sounds.play("type");
        results
    }

    fn clipboard_items(&self, search: &str) -> Vec<Item> {
        let items: Vec<Item> = self
            .clipboard
            .lock()
            .unwrap()
            .items()
            .iter()
            .enumerate()
            .map(|(index, text)| Item {
                name: text.clone(),
                shortcut: (index + 1).to_string(),
                id: uuid::Uuid::new_v4().to_string(),
                kind: ItemKind::Clip,
            })
            .collect();
        self.shortcut_then_fuzzy(search, items, true)
    }

    fn steam_items(&mut self, search: &str) -> Vec<Item> {
        if self.steam_games.is_empty() {
            self.rescan_steam();
        }
        let items: Vec<Item> = self
            .steam_games
            .iter()
            .map(|g| Item {
                name: g.name.clone(),
                shortcut: String::new(),
                id: uuid::Uuid::new_v4().to_string(),
                kind: ItemKind::Steam { appid: g.appid.clone() },
            })
            .collect();
        if search.is_empty() {
            return items;
        }
        let results = fuzzy_search(search, items, |i| i.name.clone());
        self.sounds.play("type");
        results
    }

    fn screenshot_items(&self, search: &str) -> Vec<Item> {
        let actions: [(&str, &'static str); 8] = [
            ("screenshot window to clipboard", "window"),
            ("screenshot entire screen to clipboard", "screen"),
            ("describe active window", "describe_window"),
            ("describe entire screen", "describe_screen"),
            ("explore regions of active window", "regions_window"),
            ("explore regions of entire screen", "regions_screen"),
            ("grab specific region of active window", "grab_window"),
            ("grab specific region of entire screen", "grab_screen"),
        ];
        let items: Vec<Item> = actions
            .iter()
            .enumerate()
            .map(|(index, (msgid, action))| Item {
                name: tr(msgid),
                shortcut: (index + 1).to_string(),
                id: String::new(),
                kind: ItemKind::Screenshot { action },
            })
            .collect();
        // A number key jumps straight to that action; any other text keeps
        // the full list because "grab specific region" reads the input field
        // as the element to find — typed text must not filter these away.
        if let Some(index) = exact_shortcut_match(search, &items, |i| i.shortcut.clone()) {
            self.sounds.play("match");
            return vec![items[index].clone()];
        }
        items
    }

    fn timer_items(&self, search: &str) -> Vec<Item> {
        let now = self.clock.now();
        let engine = self.timers.engine.lock().unwrap();
        let items: Vec<Item> = engine
            .timers
            .iter()
            .map(|t| Item {
                name: engine.item_label(t, now),
                shortcut: String::new(),
                id: t.id.clone(),
                kind: ItemKind::Timer,
            })
            .collect();
        drop(engine);
        if search.is_empty() {
            return items;
        }
        let results = fuzzy_search(search, items, |i| i.name.clone());
        self.sounds.play("type");
        results
    }

    fn alarm_items(&self, search: &str) -> Vec<Item> {
        let engine = self.alarms.engine.lock().unwrap();
        let items: Vec<Item> = engine
            .alarms
            .iter()
            .map(|a| Item {
                name: AlarmEngine::item_label(a),
                shortcut: String::new(),
                id: a.id.clone(),
                kind: ItemKind::Alarm,
            })
            .collect();
        drop(engine);
        if search.is_empty() {
            return items;
        }
        let results = fuzzy_search(search, items, |i| i.name.clone());
        self.sounds.play("type");
        results
    }

    fn realtime_items(&self, search: &str) -> Vec<Item> {
        let items: Vec<Item> = launchtype_core::realtime::realtime_items()
            .into_iter()
            .map(|item| Item {
                name: item.name,
                shortcut: item.shortcut.to_string(),
                id: item.id.to_string(),
                kind: ItemKind::Realtime { key: item.key.to_string() },
            })
            .collect();
        self.shortcut_then_fuzzy(search, items, true)
    }

    fn stats_items(&self) -> Vec<Item> {
        stats_labels(&self.commands.file)
            .into_iter()
            .map(|label| Item {
                name: label,
                shortcut: String::new(),
                id: String::new(),
                kind: ItemKind::Stat,
            })
            .collect()
    }

    fn ssh_items(&self) -> Vec<Item> {
        self.ssh_output
            .iter()
            .map(|line| Item {
                name: line.clone(),
                shortcut: String::new(),
                id: String::new(),
                kind: ItemKind::SshOutput,
            })
            .collect()
    }

    fn region_items(&self, search: &str) -> Vec<Item> {
        let items: Vec<Item> = self
            .regions
            .iter()
            .map(|(label, r#box)| Item {
                name: label.clone(),
                shortcut: String::new(),
                id: String::new(),
                kind: ItemKind::Region { r#box: *r#box },
            })
            .collect();
        if search.is_empty() {
            return items;
        }
        let results = fuzzy_search(search, items, |i| i.name.clone());
        self.sounds.play("type");
        results
    }
}
