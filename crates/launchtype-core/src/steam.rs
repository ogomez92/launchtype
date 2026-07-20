//! Steam appmanifest (.acf) parsing — pure port of the extraction in
//! `services/steam_scanner.py` (regexes `"appid"\s+"(\d+)"` and
//! `"name"\s+"([^"]+)"`). Directory scanning lives in `launchtype-services`.

#[derive(Debug, Clone, PartialEq)]
pub struct SteamGame {
    /// Lowercased for matching, like every launcher item name.
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
    Some(SteamGame { name: name.to_lowercase(), appid: appid.to_string() })
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
    fn parses_appid_and_lowercased_name() {
        let game = parse_appmanifest(MANIFEST).unwrap();
        assert_eq!(game.appid, "620");
        assert_eq!(game.name, "portal 2");
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
}
