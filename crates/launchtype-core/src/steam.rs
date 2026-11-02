//! Steam appmanifest (.acf) parsing — pure port of the extraction in
//! `services/steam_scanner.py` (regexes `"appid"\s+"(\d+)"` and
//! `"name"\s+"([^"]+)"`). Directory scanning lives in `launchtype-services`.
//!
//! Also the pure half of `;` mode, the games the account owns that are not
//! installed: Steam's own `loginusers.vdf` and `libraryfolders.vdf`, and the
//! answer the Web API gives to `GetOwnedGames`.

use std::collections::HashSet;
use std::iter::Peekable;
use std::str::Chars;

/// The file the shortcuts given to Steam games are kept in, beside
/// `commands.json`. It holds an [`crate::apps::AppShortcuts`] keyed by appid,
/// which, unlike the manifest's name, never changes.
pub const SHORTCUTS_FILE: &str = "steam_shortcuts.json";

#[derive(Debug, Clone, PartialEq)]
pub struct SteamGame {
    /// Spelled the way the manifest spells it; matching folds case itself.
    pub name: String,
    pub appid: String,
}

/// Extract the first quoted value following `"key"` + whitespace.
fn acf_value<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let mut search_from = 0;
    while let Some(rel) = content[search_from..].find(&needle) {
        let after_key = search_from + rel + needle.len();
        let rest = &content[after_key..];
        let trimmed = rest.trim_start();
        // The regex requires at least one whitespace char between key and value.
        if trimmed.len() != rest.len() && trimmed.starts_with('"') {
            let value = &trimmed[1..];
            if let Some(end) = value.find('"') {
                return Some(&value[..end]);
            }
        }
        search_from = after_key;
    }
    None
}

/// Parse one appmanifest_*.acf; `None` when appid/name are missing (or the
/// appid is not numeric, mirroring the `\d+` regex).
pub fn parse_appmanifest(content: &str) -> Option<SteamGame> {
    let appid = acf_value(content, "appid")?;
    if appid.is_empty() || !appid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let name = acf_value(content, "name")?;
    if name.is_empty() {
        return None;
    }
    Some(SteamGame { name: name.to_string(), appid: appid.to_string() })
}

/// A KeyValues ("VDF") value: Steam's config files are quoted keys, each
/// followed by a quoted string or a braced block of more of the same.
#[derive(Debug, Clone, PartialEq)]
enum Vdf {
    Str(String),
    Block(Vec<(String, Vdf)>),
}

impl Vdf {
    /// The value under `key`, which Steam itself spells in whatever case it
    /// likes (`Timestamp` in one release, `timestamp` in the next).
    fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Block(entries) => {
                entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
            }
            Vdf::Str(_) => None,
        }
    }

    fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Vdf::Str(value) => Some(value),
            Vdf::Block(_) => None,
        }
    }

    fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Block(entries) => entries,
            Vdf::Str(_) => &[],
        }
    }
}

/// Parse a whole VDF file into one block. Malformed input yields whatever
/// parsed before the damage, which is the most a config file read for hints
/// can usefully give.
fn parse_vdf(content: &str) -> Vdf {
    let mut chars = content.chars().peekable();
    Vdf::Block(parse_vdf_block(&mut chars))
}

fn parse_vdf_block(chars: &mut Peekable<Chars>) -> Vec<(String, Vdf)> {
    let mut entries = Vec::new();
    let mut key: Option<String> = None;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let text = parse_vdf_string(chars);
                match key.take() {
                    None => key = Some(text),
                    Some(k) => entries.push((k, Vdf::Str(text))),
                }
            }
            '{' => {
                let block = parse_vdf_block(chars);
                if let Some(k) = key.take() {
                    entries.push((k, Vdf::Block(block)));
                }
            }
            '}' => break,
            // `//` comments run to the end of the line.
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    entries
}

/// The rest of a quoted string, the opening quote already taken. Backslash
/// escapes are undone: `libraryfolders.vdf` writes `S:\\SteamLibrary`.
fn parse_vdf_string(chars: &mut Peekable<Chars>) -> String {
    let mut text = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                Some('n') => text.push('\n'),
                Some('t') => text.push('\t'),
                Some(other) => text.push(other),
                None => break,
            },
            _ => text.push(c),
        }
    }
    text
}

fn is_steam_id(text: &str) -> bool {
    text.len() == 17 && text.bytes().all(|b| b.is_ascii_digit())
}

/// The SteamID64 of the account that last signed in to this Steam, from
/// `config/loginusers.vdf`: the one marked `MostRecent`, or failing that the
/// one with the latest timestamp. `None` when no account has signed in here.
pub fn most_recent_steam_id(loginusers: &str) -> Option<String> {
    let root = parse_vdf(loginusers);
    let accounts = root.get("users")?.entries().iter().filter(|(id, _)| is_steam_id(id));
    if let Some((id, _)) = accounts.clone().find(|(_, user)| user.str("MostRecent") == Some("1")) {
        return Some(id.clone());
    }
    accounts
        .max_by_key(|(_, user)| user.str("Timestamp").and_then(|t| t.parse::<u64>().ok()).unwrap_or(0))
        .map(|(id, _)| id.clone())
}

/// Every library folder `libraryfolders.vdf` lists, as Steam wrote the path
/// (the folder holding `steamapps`, not `steamapps` itself).
pub fn library_folder_paths(libraryfolders: &str) -> Vec<String> {
    let root = parse_vdf(libraryfolders);
    let Some(folders) = root.get("libraryfolders") else { return Vec::new() };
    folders
        .entries()
        .iter()
        .filter_map(|(_, folder)| folder.str("path"))
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
}

/// The games in a `GetOwnedGames` answer (asked with `include_appinfo`, so
/// each carries its name). `None` when the body is not that answer at all.
/// An empty list is a real answer: an account that owns nothing, or one
/// whose game details are private, which Steam does not tell apart.
pub fn parse_owned_games(body: &str) -> Option<Vec<SteamGame>> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let response = value.get("response")?.as_object()?;
    let Some(games) = response.get("games") else { return Some(Vec::new()) };
    let games = games
        .as_array()?
        .iter()
        .filter_map(|game| {
            let appid = game.get("appid")?.as_u64()?.to_string();
            // An app Steam has no name for is one nobody could pick out of a
            // list anyway.
            let name = game.get("name")?.as_str()?.trim();
            (!name.is_empty()).then(|| SteamGame { name: name.to_string(), appid })
        })
        .collect();
    Some(games)
}

/// The owned games whose appids are not among `installed`, alphabetically.
pub fn not_installed(owned: Vec<SteamGame>, installed: &HashSet<String>) -> Vec<SteamGame> {
    let mut games: Vec<SteamGame> =
        owned.into_iter().filter(|game| !installed.contains(&game.appid)).collect();
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// How the user named their Steam account, when it could not be read off
/// this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteamAccount {
    /// A SteamID64, which the Web API takes as it is.
    Id(String),
    /// The custom part of a `steamcommunity.com/id/...` URL, which has to be
    /// turned into an ID first.
    Vanity(String),
}

/// Read what was typed for the account: a SteamID64, a profile URL of either
/// kind, or a bare custom URL name. `None` when nothing usable was typed.
pub fn parse_steam_account(input: &str) -> Option<SteamAccount> {
    let input = input.trim();
    let name = ["/profiles/", "/id/"]
        .iter()
        .find_map(|marker| input.find(marker).map(|at| &input[at + marker.len()..]))
        .map(|rest| rest.split('/').next().unwrap_or(""))
        .unwrap_or(input.trim_end_matches('/'));
    if name.is_empty() {
        return None;
    }
    Some(if is_steam_id(name) {
        SteamAccount::Id(name.to_string())
    } else {
        SteamAccount::Vanity(name.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"
"AppState"
{
	"appid"		"620"
	"Universe"		"1"
	"name"		"Portal 2"
	"StateFlags"		"4"
	"installdir"		"Portal 2"
}
"#;

    #[test]
    fn parses_appid_and_name() {
        let game = parse_appmanifest(MANIFEST).unwrap();
        assert_eq!(game.appid, "620");
        assert_eq!(game.name, "Portal 2");
    }

    #[test]
    fn missing_fields_yield_none() {
        assert!(parse_appmanifest("\"appid\"\t\"620\"").is_none());
        assert!(parse_appmanifest("\"name\"\t\"Portal 2\"").is_none());
        assert!(parse_appmanifest("").is_none());
    }

    #[test]
    fn non_numeric_appid_rejected() {
        let content = "\"appid\"\t\"abc\"\n\"name\"\t\"Game\"";
        assert!(parse_appmanifest(content).is_none());
    }

    #[test]
    fn key_without_whitespace_separator_is_skipped() {
        // The Python regex needs \s+ between key and value.
        assert!(acf_value("\"appid\"\"620\"", "appid").is_none());
        assert_eq!(acf_value("\"appid\"  \"620\"", "appid"), Some("620"));
    }

    const LOGINUSERS: &str = r#"
"users"
{
	"76561198000000001"
	{
		"AccountName"		"old"
		"MostRecent"		"0"
		"Timestamp"		"1700000000"
	}
	"76561198000000002"
	{
		"AccountName"		"new"
		"MostRecent"		"1"
		"Timestamp"		"1600000000"
	}
}
"#;

    #[test]
    fn the_most_recent_account_is_the_one_steam_marks() {
        assert_eq!(most_recent_steam_id(LOGINUSERS).as_deref(), Some("76561198000000002"));
    }

    /// Recent Steam clients drop `MostRecent` and lower-case the timestamp.
    #[test]
    fn without_a_mark_the_latest_timestamp_wins() {
        let content = LOGINUSERS
            .replace("\"MostRecent\"\t\t\"1\"", "")
            .replace("Timestamp", "timestamp");
        assert_eq!(most_recent_steam_id(&content).as_deref(), Some("76561198000000001"));
        assert_eq!(most_recent_steam_id("\"users\"\n{\n}\n"), None);
        assert_eq!(most_recent_steam_id(""), None);
    }

    #[test]
    fn library_folders_are_read_with_their_backslashes_undone() {
        let content = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps"
		{
			"228980"		"0"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}
"#;
        assert_eq!(
            library_folder_paths(content),
            [r"C:\Program Files (x86)\Steam", r"D:\SteamLibrary"]
        );
        assert!(library_folder_paths("garbage").is_empty());
    }

    #[test]
    fn owned_games_come_with_their_names() {
        let body = r#"{"response":{"game_count":3,"games":[
            {"appid":620,"name":"Portal 2","playtime_forever":5},
            {"appid":440,"name":"Team Fortress 2"},
            {"appid":999,"name":"  "}]}}"#;
        let games = parse_owned_games(body).unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0], SteamGame { name: "Portal 2".into(), appid: "620".into() });
    }

    #[test]
    fn a_private_or_empty_account_is_an_empty_list_and_junk_is_none() {
        assert_eq!(parse_owned_games(r#"{"response":{}}"#), Some(Vec::new()));
        assert_eq!(parse_owned_games("<html>"), None);
        assert_eq!(parse_owned_games(r#"{"other":1}"#), None);
    }

    #[test]
    fn installed_games_are_left_out_and_the_rest_sorted() {
        let game = |name: &str, appid: &str| SteamGame { name: name.into(), appid: appid.into() };
        let owned = vec![game("portal 2", "620"), game("Doom", "2280"), game("Hexen", "2360")];
        let installed: HashSet<String> = ["2360".to_string()].into();
        let names: Vec<_> = not_installed(owned, &installed).into_iter().map(|g| g.name).collect();
        assert_eq!(names, ["Doom", "portal 2"]);
    }

    #[test]
    fn an_account_can_be_typed_several_ways() {
        let id = || Some(SteamAccount::Id("76561198983806636".into()));
        assert_eq!(parse_steam_account("76561198983806636"), id());
        assert_eq!(
            parse_steam_account("https://steamcommunity.com/profiles/76561198983806636/"),
            id()
        );
        assert_eq!(
            parse_steam_account(" https://steamcommunity.com/id/gabe/games "),
            Some(SteamAccount::Vanity("gabe".into()))
        );
        assert_eq!(parse_steam_account("gabe"), Some(SteamAccount::Vanity("gabe".into())));
        assert_eq!(parse_steam_account("  "), None);
        assert_eq!(parse_steam_account("https://steamcommunity.com/id/"), None);
    }
}
