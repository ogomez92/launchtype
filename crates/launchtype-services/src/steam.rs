//! Steam library scanning — the directory walk of `services/steam_scanner.py`
//! (the .acf parsing itself lives in `launchtype_core::steam`) — and the Steam
//! Web API calls `;` mode makes to learn which games the account owns.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use launchtype_core::i18n::{format_args, tr, Arg};
use launchtype_core::steam::{
    library_folder_paths, most_recent_steam_id, parse_appmanifest, parse_installed_game,
    parse_owned_games, sort_largest_first, InstalledGame, SteamAccount, SteamGame,
};

const API_TIMEOUT: Duration = Duration::from_secs(20);

/// Scan `library_path` for `appmanifest_*.acf` files; unreadable directories
/// or files are skipped. Games come back sorted alphabetically by name.
pub fn scan_games(library_path: &Path) -> Vec<SteamGame> {
    let mut games: Vec<SteamGame> =
        manifests(library_path).iter().filter_map(|content| parse_appmanifest(content)).collect();
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// The contents of every readable `appmanifest_*.acf` in one `steamapps` folder.
fn manifests(library_path: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(library_path) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.starts_with("appmanifest_") && name.ends_with(".acf")
            })
        })
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .collect()
}

/// The games installed in any of Steam's libraries (see [`library_folders`]),
/// each with the size its manifest records, largest first. Reading the size
/// off the manifest costs nothing beyond the scan `,` already does.
pub fn installed_games_by_size(library_path: &Path) -> Vec<InstalledGame> {
    let mut seen = HashSet::new();
    let mut games: Vec<InstalledGame> = library_folders(library_path)
        .iter()
        .flat_map(|folder| manifests(folder))
        .filter_map(|content| parse_installed_game(&content))
        .filter(|installed| seen.insert(installed.game.appid.clone()))
        .collect();
    sort_largest_first(&mut games);
    games
}

/// The steam:// URL that launches a game.
pub fn rungameid_url(appid: &str) -> String {
    format!("steam://rungameid/{appid}")
}

/// The steam:// URL that opens Steam's install dialog for a game.
pub fn install_url(appid: &str) -> String {
    format!("steam://install/{appid}")
}

/// The steam:// URL that opens Steam's own uninstall confirmation for a game.
pub fn uninstall_url(appid: &str) -> String {
    format!("steam://uninstall/{appid}")
}

/// Every library this Steam installs into, as `steamapps` folders: the one
/// configured, then the others its `libraryfolders.vdf` lists. Without the
/// others a game installed on a second drive would be offered for install.
pub fn library_folders(library_path: &Path) -> Vec<PathBuf> {
    let mut folders = vec![library_path.to_path_buf()];
    // The list sits in `steamapps` itself, and in `config` beside it.
    let lists = [
        Some(library_path.join("libraryfolders.vdf")),
        library_path.parent().map(|root| root.join("config").join("libraryfolders.vdf")),
    ];
    for list in lists.into_iter().flatten() {
        let Ok(content) = std::fs::read_to_string(list) else { continue };
        for path in library_folder_paths(&content) {
            let steamapps = PathBuf::from(path).join("steamapps");
            if !folders.iter().any(|known| same_folder(known, &steamapps)) {
                folders.push(steamapps);
            }
        }
    }
    folders
}

/// Steam writes its paths in whatever case and slashes it likes, and the
/// configured library is typed by hand.
fn same_folder(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The appids installed in any of Steam's libraries (see [`library_folders`]).
pub fn installed_appids(library_path: &Path) -> HashSet<String> {
    library_folders(library_path)
        .iter()
        .flat_map(|folder| scan_games(folder))
        .map(|game| game.appid)
        .collect()
}

/// The account that last signed in to the Steam `library_path` belongs to,
/// read from its `config/loginusers.vdf`. `None` when the library is not the
/// one inside Steam's own folder, or nobody has signed in.
pub fn signed_in_account(library_path: &Path) -> Option<String> {
    let root = library_path.parent()?;
    let content = std::fs::read_to_string(root.join("config").join("loginusers.vdf")).ok()?;
    most_recent_steam_id(&content)
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct SteamApiError {
    pub message: String,
    /// Steam turned the key down, so the stored one is no use.
    pub key_rejected: bool,
}

impl SteamApiError {
    fn new(message: String) -> Self {
        SteamApiError { message, key_rejected: false }
    }
}

/// Ask one Web API method, hand back the body.
fn call(method: &str, query: &[(&str, &str)]) -> Result<String, SteamApiError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(API_TIMEOUT)
        .user_agent(crate::USER_AGENT)
        .build();
    let mut request = agent.get(&format!("https://api.steampowered.com/{method}"));
    for (key, value) in query {
        request = request.query(key, value);
    }
    match request.call() {
        Ok(response) => response
            .into_string()
            .map_err(|error| SteamApiError::new(network_error(&error.to_string()))),
        Err(ureq::Error::Status(401 | 403, _)) => Err(SteamApiError {
            message: tr("Steam refused the Web API key. Enter it again."),
            key_rejected: true,
        }),
        Err(ureq::Error::Status(code, _)) => Err(SteamApiError::new(format_args(
            &tr("Steam answered with status {code}."),
            &[("code", Arg::Int(code as i64))],
        ))),
        Err(ureq::Error::Transport(t)) => Err(SteamApiError::new(network_error(&t.to_string()))),
    }
}

fn network_error(detail: &str) -> String {
    tr("Network error: {}").replacen("{}", detail, 1)
}

/// Turn what the user typed for their account into a SteamID64, asking Steam
/// when it is a custom URL name.
pub fn resolve_account(api_key: &str, account: &SteamAccount) -> Result<String, SteamApiError> {
    let vanity = match account {
        SteamAccount::Id(id) => return Ok(id.clone()),
        SteamAccount::Vanity(vanity) => vanity,
    };
    let body = call("ISteamUser/ResolveVanityURL/v1/", &[("key", api_key), ("vanityurl", vanity)])?;
    let value: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let response = &value["response"];
    match (response["success"].as_i64(), response["steamid"].as_str()) {
        (Some(1), Some(id)) => Ok(id.to_string()),
        _ => Err(SteamApiError::new(format_args(
            &tr("Steam knows no account called {name}. Check the Steam account in Settings."),
            &[("name", Arg::Str(vanity))],
        ))),
    }
}

/// Every game the account owns, free ones it has played included. Steam
/// leaves out games it has not reviewed for its store ("unvetted") and ones
/// claimed as free licenses unless asked for them — a good share of the
/// small audio games among them — so both are asked for.
pub fn owned_games(api_key: &str, steam_id: &str) -> Result<Vec<SteamGame>, SteamApiError> {
    let body = call(
        "IPlayerService/GetOwnedGames/v1/",
        &[
            ("key", api_key),
            ("steamid", steam_id),
            ("include_appinfo", "1"),
            ("include_played_free_games", "1"),
            ("skip_unvetted_apps", "0"),
            ("include_free_sub", "1"),
            ("format", "json"),
        ],
    )?;
    let games = parse_owned_games(&body)
        .ok_or_else(|| SteamApiError::new(tr("Steam did not send the list of games.")))?;
    if games.is_empty() {
        return Err(SteamApiError::new(tr(
            "Steam listed no games for this account. If it owns some, set the game details of its profile to public.",
        )));
    }
    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_and_sorts_manifests() {
        let dir = tempfile::tempdir().unwrap();
        let write = |file: &str, appid: &str, name: &str| {
            std::fs::write(
                dir.path().join(file),
                format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"name\"\t\t\"{name}\"\n}}\n"),
            )
            .unwrap();
        };
        write("appmanifest_620.acf", "620", "Portal 2");
        write("appmanifest_440.acf", "440", "Team Fortress 2");
        std::fs::write(dir.path().join("libraryfolders.vdf"), "ignored").unwrap();

        let games = scan_games(dir.path());
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].name, "Portal 2");
        assert_eq!(games[1].name, "Team Fortress 2");
        assert_eq!(rungameid_url(&games[0].appid), "steam://rungameid/620");
    }

    #[test]
    fn missing_library_yields_empty() {
        assert!(scan_games(Path::new("Z:/no/such/dir")).is_empty());
    }

    #[test]
    fn every_library_steam_lists_counts_as_installed() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let steamapps = root.path().join("steamapps");
        let other_steamapps = other.path().join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        std::fs::create_dir_all(&other_steamapps).unwrap();
        let manifest = |dir: &Path, appid: &str| {
            std::fs::write(
                dir.join(format!("appmanifest_{appid}.acf")),
                format!("\"appid\"\t\"{appid}\"\n\"name\"\t\"Game {appid}\"\n"),
            )
            .unwrap();
        };
        manifest(&steamapps, "620");
        manifest(&other_steamapps, "440");
        // Steam lists its own folder too; it must not be scanned twice over.
        let escape = |path: &Path| path.display().to_string().replace('\\', "\\\\");
        std::fs::write(
            steamapps.join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\"0\"\n{{\n\"path\"\t\"{}\"\n}}\n\"1\"\n{{\n\"path\"\t\"{}\"\n}}\n}}\n",
                escape(root.path()),
                escape(other.path()),
            ),
        )
        .unwrap();

        assert_eq!(library_folders(&steamapps).len(), 2);
        let installed = installed_appids(&steamapps);
        assert_eq!(installed, ["620".to_string(), "440".to_string()].into());

        std::fs::write(
            other_steamapps.join("appmanifest_440.acf"),
            "\"appid\"	\"440\"
\"name\"	\"Game 440\"
\"SizeOnDisk\"	\"900\"
",
        )
        .unwrap();
        let by_size: Vec<_> = installed_games_by_size(&steamapps)
            .into_iter()
            .map(|installed| (installed.game.appid, installed.size))
            .collect();
        assert_eq!(by_size, [("440".to_string(), Some(900)), ("620".to_string(), None)]);
        assert_eq!(uninstall_url("440"), "steam://uninstall/440");
    }

    #[test]
    fn the_signed_in_account_is_read_from_steams_config() {
        let root = tempfile::tempdir().unwrap();
        let steamapps = root.path().join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        assert_eq!(signed_in_account(&steamapps), None);
        std::fs::create_dir_all(root.path().join("config")).unwrap();
        std::fs::write(
            root.path().join("config").join("loginusers.vdf"),
            "\"users\"\n{\n\"76561198000000002\"\n{\n\"MostRecent\"\t\"1\"\n}\n}\n",
        )
        .unwrap();
        assert_eq!(signed_in_account(&steamapps).as_deref(), Some("76561198000000002"));
        assert_eq!(install_url("620"), "steam://install/620");
    }
}
