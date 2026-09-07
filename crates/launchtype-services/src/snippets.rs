//! Snippet loading — `snippets/*.txt` files (filename = shortcut) plus the
//! read-only Apple `apple_snippets.plist` (ports `helpers/plist_helper.py`
//! and `DataManager.load_snippets_from_files`).

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Snippet {
    /// Lowercase shortcut (for txt snippets, the filename up to the first dot).
    pub shortcut: String,
    pub contents: String,
}

/// Parse Apple snippets; a missing or malformed file yields an empty list.
/// A leading dash on a shortcut is stripped.
pub fn parse_apple_snippets(path: &Path) -> Vec<Snippet> {
    let Ok(value) = plist::Value::from_file(path) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let dict = item.as_dictionary()?;
            let shortcut = dict.get("shortcut")?.as_string()?;
            let phrase = dict.get("phrase")?.as_string()?;
            let shortcut = shortcut.strip_prefix('-').unwrap_or(shortcut);
            Some(Snippet { shortcut: shortcut.to_string(), contents: phrase.to_string() })
        })
        .collect()
}

/// Load all snippets from `working_dir`: Apple snippets first, then the
/// `snippets/` folder (created if missing), matching the Python load order.
pub fn load_snippets(working_dir: &Path) -> Vec<Snippet> {
    let mut snippets = parse_apple_snippets(&working_dir.join("apple_snippets.plist"));

    let dir = working_dir.join("snippets");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // The user's placeholders share this folder but are not a snippet;
            // without this they would show up in the list as "placeholders",
            // pasting their own JSON.
            if file_name.eq_ignore_ascii_case(launchtype_core::placeholders::FILE_NAME) {
                continue;
            }
            let shortcut = file_name.split('.').next().unwrap_or(file_name).to_lowercase();
            if let Ok(contents) = std::fs::read_to_string(&path) {
                snippets.push(Snippet { shortcut, contents });
            }
        }
    }
    snippets
}

/// Whether `name` is usable as a snippet's file name.
///
/// A shortcut is a word the user types to summon a snippet, and it is also,
/// directly, a file name: `snippets/<name>.txt`. Nothing stopped a name from
/// carrying `..` or a separator, which made "add a snippet" a way to write a
/// chosen file anywhere the user can write — `../../commands.json`, or a
/// script into the Startup folder — and made the rename half of
/// [`update_snippet`] a way to *delete* one.
///
/// It is not only the user's own typing that gets here: shortcuts also come
/// out of `apple_snippets.plist`, which the app reads but does not write.
///
/// So a name must be one plain file-name component: no separators, no drive
/// letter, no `.`/`..`, and none of the characters Windows refuses in a name
/// anyway.
pub fn is_valid_shortcut(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\0'])
        // Windows drops a trailing dot from a file name, so `sig.` and `sig`
        // would be one file wearing two shortcuts. (Trailing spaces, which it
        // drops too, are gone already: the name is trimmed before it is used,
        // so `"sig "` and `"sig"` are the same shortcut by then.)
        && !name.ends_with('.')
}

fn checked_path(name: &str) -> std::io::Result<std::path::PathBuf> {
    if !is_valid_shortcut(name) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{name:?} is not a usable snippet name"),
        ));
    }
    Ok(std::path::Path::new("snippets").join(format!("{}.txt", name.trim())))
}

/// Write (or overwrite) `snippets/<name>.txt` (DataManager.add_snippet).
pub fn write_snippet(name: &str, contents: &str) -> std::io::Result<()> {
    let path = checked_path(name)?;
    std::fs::create_dir_all("snippets")?;
    std::fs::write(path, contents)
}

/// Rename-aware snippet update: removes the old file when the snippet was
/// renamed so no stale duplicate is left behind, then writes the new one.
pub fn update_snippet(original_shortcut: &str, name: &str, contents: &str) -> std::io::Result<()> {
    // The new name is checked before the old file is touched, so a rejected
    // rename does not delete the snippet it was renaming.
    let new_path = checked_path(name)?;
    if !original_shortcut.is_empty() && !original_shortcut.eq_ignore_ascii_case(name) {
        match checked_path(original_shortcut) {
            Ok(old) if old.exists() => {
                let _ = std::fs::remove_file(old);
            }
            Ok(_) => {}
            // A stored shortcut that is not a usable file name names no file
            // this wrote; there is nothing to remove and nothing to guess at.
            Err(e) => log::warn!("not removing the old snippet file: {e}"),
        }
    }
    std::fs::create_dir_all("snippets")?;
    std::fs::write(new_path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shortcut is the file name, so it has to name a file inside
    /// `snippets/` and nowhere else.
    #[test]
    fn a_snippet_name_cannot_reach_out_of_the_folder() {
        let bad = [
            "../commands",
            r"..\commands",
            "..",
            ".",
            "sub/dir",
            r"sub\dir",
            r"C:\Windows\evil",
            "trailing.",
            "",
            "   ",
            "a\"quote",
        ];
        for name in bad {
            assert!(!is_valid_shortcut(name), "{name:?} should be refused");
            assert!(write_snippet(name, "x").is_err(), "{name:?} should not be written");
            // And the rename path must refuse it before removing anything.
            assert!(update_snippet("sig", name, "x").is_err(), "{name:?} should not be written");
        }
    }

    #[test]
    fn ordinary_shortcuts_are_still_accepted() {
        for good in ["sig", "my.note", "correo-trabajo", "a b", "firma"] {
            assert!(is_valid_shortcut(good), "{good:?} should be accepted");
        }
        // Stray spaces around a name are trimmed rather than refused.
        assert!(is_valid_shortcut("  sig  "));
    }

    #[test]
    fn txt_snippets_use_filename_up_to_first_dot_lowercased() {
        let dir = tempfile::tempdir().unwrap();
        let snippets_dir = dir.path().join("snippets");
        std::fs::create_dir(&snippets_dir).unwrap();
        std::fs::write(snippets_dir.join("Sig.txt"), "Best regards,\nOscar").unwrap();
        std::fs::write(snippets_dir.join("my.note.txt"), "note body").unwrap();

        let mut snippets = load_snippets(dir.path());
        snippets.sort_by(|a, b| a.shortcut.cmp(&b.shortcut));
        assert_eq!(
            snippets,
            vec![
                Snippet { shortcut: "my".into(), contents: "note body".into() },
                Snippet { shortcut: "sig".into(), contents: "Best regards,\nOscar".into() },
            ]
        );
    }

    /// `placeholders.json` lives in this folder and is not a snippet.
    #[test]
    fn the_placeholders_file_is_not_loaded_as_a_snippet() {
        let dir = tempfile::tempdir().unwrap();
        let snippets_dir = dir.path().join("snippets");
        std::fs::create_dir(&snippets_dir).unwrap();
        std::fs::write(snippets_dir.join("placeholders.json"), r#"{"hi": "hola"}"#).unwrap();
        std::fs::write(snippets_dir.join("sig.txt"), "Oscar").unwrap();

        assert_eq!(
            load_snippets(dir.path()),
            vec![Snippet { shortcut: "sig".into(), contents: "Oscar".into() }]
        );
    }

    #[test]
    fn snippets_dir_is_created_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_snippets(dir.path()).is_empty());
        assert!(dir.path().join("snippets").is_dir());
    }

    #[test]
    fn apple_snippets_parse_with_dash_stripping() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("apple_snippets.plist");
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<array>
  <dict>
    <key>phrase</key><string>my@email.example</string>
    <key>shortcut</key><string>-em</string>
  </dict>
  <dict>
    <key>phrase</key><string>plain phrase</string>
    <key>shortcut</key><string>pp</string>
  </dict>
</array>
</plist>"#;
        std::fs::write(&path, xml).unwrap();
        let snippets = parse_apple_snippets(&path);
        assert_eq!(
            snippets,
            vec![
                Snippet { shortcut: "em".into(), contents: "my@email.example".into() },
                Snippet { shortcut: "pp".into(), contents: "plain phrase".into() },
            ]
        );
        assert!(parse_apple_snippets(&dir.path().join("missing.plist")).is_empty());
    }
}
