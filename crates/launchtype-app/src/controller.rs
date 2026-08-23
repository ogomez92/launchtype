//! ModeController — the Rust counterpart of `DataManager`'s per-mode item
//! dispatch: owns the data stores and answers "what does the list show for
//! this mode + search text", including the match/type sound cues.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use launchtype_core::alarms::AlarmEngine;
use launchtype_core::clipboard_history::ClipboardHistory;
use launchtype_core::clock::{Clock, SystemClock};
use launchtype_core::emoji;
use launchtype_core::i18n::tr;
use launchtype_core::mode::UiMode;
use launchtype_core::portable;
use launchtype_core::search::{exact_shortcut_match, fuzzy_search, keyword_query_match};
use launchtype_core::stats::stats_labels;
use launchtype_core::units;
use launchtype_core::vault::VaultSession;
use launchtype_services::snippets::{load_snippets, Snippet};
use launchtype_services::sounds::SoundPlayer;
use launchtype_services::steam::scan_games;
use launchtype_services::stores::{AlarmStore, CommandsStore, TimerStore};

/// How many emoji the list will show at once. There are close to two thousand,
/// and a single common letter matches most of them; past a couple of hundred
/// rows the list is neither navigable nor quick to redraw on every keystroke,
/// and the best matches are at the top anyway.
const EMOJI_LIMIT: usize = 200;

fn vault_action_item(name: String, action: &'static str) -> Item {
    Item { name, shortcut: String::new(), id: String::new(), kind: ItemKind::VaultAction { action } }
}

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
    /// The characters to copy; the item's name is the emoji's spoken name.
    Emoji { emoji: &'static str },
    /// One unit conversion; the item's name is the whole sentence the list
    /// shows and `result` is the number alone, which is what Enter copies.
    /// `None` until a number has been typed.
    Conversion { result: Option<String> },
    Stat,
    Region { r#box: [f64; 4] },
    /// One line of SSH command output (or of the echoed command line).
    SshOutput,
    /// One entry of the encrypted vault, identified by `Item::id`. The secret
    /// is deliberately absent: rows are rebuilt on every keystroke and read
    /// out loud, so the secret is decrypted only when Enter copies it.
    VaultEntry,
    /// A vault row that does something rather than holding a secret: set up,
    /// unlock, lock, add, or change the master password.
    VaultAction { action: &'static str },
}

pub struct ModeController {
    pub commands: CommandsStore,
    pub sort_by_uses: bool,
    pub snippets: Vec<Snippet>,
    pub clipboard: Arc<Mutex<ClipboardHistory>>,
    /// Shared with the auto-lock thread, which wipes the key on idle.
    pub vault: Arc<Mutex<VaultSession>>,
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        commands: CommandsStore,
        sort_by_uses: bool,
        clipboard: Arc<Mutex<ClipboardHistory>>,
        vault: Arc<Mutex<VaultSession>>,
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
            vault,
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
            UiMode::Emoji => self.emoji_items(search),
            UiMode::Units => self.conversion_items(search),
            UiMode::Vault => self.vault_items(search),
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

        // Keyword search: "g cats" finds the command whose shortcut is
        // exactly "g" and whose path or args names {{query}}, and bakes the
        // typed remainder into that one placeholder before the item ever
        // reaches the list — same one-item-then-Enter shape as an ordinary
        // exact-shortcut match, just with the blank filled in. A command
        // without {{query}} is not a keyword-search command, so it is left
        // alone here and "d something" falls through to the plain
        // shortcut/fuzzy search below untouched.
        if let Some((index, query)) = keyword_query_match(search, &items, |i| i.shortcut.clone()) {
            if let ItemKind::Command { path, args, run_as_admin } = items[index].kind.clone() {
                let has_query = portable::placeholder_names(&path).any(|n| n == portable::QUERY_PLACEHOLDER)
                    || portable::placeholder_names(&args).any(|n| n == portable::QUERY_PLACEHOLDER);
                if has_query {
                    let encoded = portable::url_encode(&query);
                    let mut item = items[index].clone();
                    item.name = format!("{} {}", item.name, query);
                    item.kind = ItemKind::Command {
                        path: portable::substitute_query(&path, &encoded),
                        args: portable::substitute_query(&args, &encoded),
                        run_as_admin,
                    };
                    self.sounds.play("match");
                    return vec![item];
                }
            }
        }

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
        // A number key jumps straight to that action, and anything else is an
        // ordinary search. "Grab specific region" used to read the input field
        // as the element to find, so typed text could not be allowed to filter
        // the list; it asks in a dialog now, and this mode behaves like the
        // rest.
        self.shortcut_then_fuzzy(search, items, true)
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

    /// Emoji, searched by name *and* by CLDR's keywords ("laugh" finds "face
    /// with tears of joy") but listed by name alone.
    ///
    /// The glyph itself is deliberately not in the label: screen readers
    /// announce emoji by the very name next to it, so showing both means
    /// hearing "grinning face grinning face" on every arrow press.
    fn emoji_items(&self, search: &str) -> Vec<Item> {
        let matches = emoji::search(&launchtype_core::i18n::language(), search);
        if !search.is_empty() {
            self.sounds.play("type");
        }
        matches
            .into_iter()
            .take(EMOJI_LIMIT)
            .map(|e| Item {
                name: e.name.to_string(),
                shortcut: String::new(),
                id: String::new(),
                kind: ItemKind::Emoji { emoji: e.emoji },
            })
            .collect()
    }

    /// Unit conversions for the number (and the units) typed so far.
    ///
    /// The typed text is not a filter over a fixed list the way it is
    /// everywhere else: the number in front of it is what every row converts,
    /// and only the words after it narrow the list down.
    fn conversion_items(&self, search: &str) -> Vec<Item> {
        if !search.is_empty() {
            self.sounds.play("type");
        }
        units::rows(search)
            .into_iter()
            .map(|row| Item {
                name: row.label,
                shortcut: String::new(),
                id: row.id,
                kind: ItemKind::Conversion { result: row.result },
            })
            .collect()
    }

    /// The encrypted vault: a single "unlock" row while locked, the entry
    /// names once open.
    ///
    /// Entry rows carry names and shortcuts only — never a secret. Searching
    /// behaves like every other mode (exact shortcut wins, then fuzzy), and
    /// the action rows are appended only when nothing is typed so they never
    /// come between the user and the entry they are looking for.
    fn vault_items(&self, search: &str) -> Vec<Item> {
        let vault = self.vault.lock().unwrap();
        if !vault.is_unlocked() {
            // Nothing to search until the key is in memory, so the typed text
            // is ignored rather than filtering the one row away.
            let (name, action) = if vault.is_new() {
                (tr("Set up the vault: choose a master password"), "create")
            } else {
                (tr("Unlock the vault"), "unlock")
            };
            return vec![vault_action_item(name, action)];
        }
        let items: Vec<Item> = vault
            .entries()
            .iter()
            .map(|entry| Item {
                name: entry.name.clone(),
                shortcut: entry.shortcut.clone(),
                id: entry.id.clone(),
                kind: ItemKind::VaultEntry,
            })
            .collect();
        let empty = items.is_empty();
        drop(vault);

        let mut items = self.shortcut_then_fuzzy(search, items, true);
        if search.is_empty() {
            if empty {
                items.push(vault_action_item(
                    tr("The vault is empty. Press Enter to put a secret in it."),
                    "add",
                ));
            }
            items.push(vault_action_item(tr("Lock the vault now"), "lock"));
            items.push(vault_action_item(tr("Change the master password"), "password"));
        }
        items
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
