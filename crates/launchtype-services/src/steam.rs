//! Steam library scanning — the directory walk of `services/steam_scanner.py`
//! (the .acf parsing itself lives in `launchtype_core::steam`).

use std::path::Path;

use launchtype_core::steam::{parse_appmanifest, SteamGame};

/// Scan `library_path` for `appmanifest_*.acf` files; unreadable directories
/// or files are skipped. Games come back sorted alphabetically by name.
pub fn scan_games(library_path: &Path) -> Vec<SteamGame> {
    let mut games = Vec::new();
    let Ok(entries) = std::fs::read_dir(library_path) else {
        return games;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            if let Some(game) = parse_appmanifest(&content) {
                games.push(game);
            }
        }
    }
    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    games
}

/// The steam:// URL that launches a game.
pub fn rungameid_url(appid: &str) -> String {
    format!("steam://rungameid/{appid}")
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
        assert_eq!(games[0].name, "portal 2");
        assert_eq!(games[1].name, "team fortress 2");
        assert_eq!(rungameid_url(&games[0].appid), "steam://rungameid/620");
    }

    #[test]
    fn missing_library_yields_empty() {
        assert!(scan_games(Path::new("Z:/no/such/dir")).is_empty());
    }
}
