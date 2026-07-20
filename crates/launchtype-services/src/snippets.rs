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
            let shortcut = file_name.split('.').next().unwrap_or(file_name).to_lowercase();
            if let Ok(contents) = std::fs::read_to_string(&path) {
                snippets.push(Snippet { shortcut, contents });
            }
        }
    }
    snippets
}

#[cfg(test)]
mod tests {
    use super::*;

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
