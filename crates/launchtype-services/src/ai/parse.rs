//! Pure reply parsing for the AI service (kept alongside the HTTP code but
//! fully unit-testable): region-array and object extraction from prose-wrapped
//! model replies, plus the Codex config model line.

#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub label: String,
    /// `[x1, y1, x2, y2]` in the coordinate space of the image sent to the AI.
    pub r#box: [f64; 4],
}

/// Parse a model reply into regions. The reply should be a JSON array;
/// models sometimes wrap it in prose or a code fence, so the first `[`..`]`
/// span is sliced out before parsing. Entries need a non-empty label and a
/// 4-number box.
pub fn extract_regions(text: &str) -> Vec<Region> {
    let Some(start) = text.find('[') else { return Vec::new() };
    let Some(end) = text.rfind(']') else { return Vec::new() };
    if end <= start {
        return Vec::new();
    }
    let Ok(raw) = serde_json::from_str::<serde_json::Value>(&text[start..=end]) else {
        return Vec::new();
    };
    let Some(entries) = raw.as_array() else { return Vec::new() };

    entries
        .iter()
        .filter_map(|entry| {
            let entry = entry.as_object()?;
            let box_value = entry.get("box")?.as_array()?;
            if box_value.len() != 4 {
                return None;
            }
            let mut r#box = [0.0; 4];
            for (i, v) in box_value.iter().enumerate() {
                r#box[i] = v.as_f64()?;
            }
            let label = entry.get("label").and_then(|l| l.as_str()).unwrap_or("").trim();
            if label.is_empty() {
                return None;
            }
            Some(Region { label: label.to_string(), r#box })
        })
        .collect()
}

/// Parse the first `{`..`}` JSON object out of a model reply.
pub fn extract_object(text: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    match serde_json::from_str::<serde_json::Value>(&text[start..=end]) {
        Ok(serde_json::Value::Object(map)) => Some(map),
        _ => None,
    }
}

/// The top-level `model = "..."` line of a Codex config.toml (stops at the
/// first table section, like the Python line scanner).
pub fn codex_model_from_config(config: &str) -> Option<String> {
    for line in config.lines() {
        let stripped = line.trim();
        if stripped.starts_with('[') {
            break;
        }
        let Some(rest) = stripped.strip_prefix("model") else { continue };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let quote = rest.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let inner = &rest[1..];
        if let Some(end) = inner.find(quote) {
            if end > 0 {
                return Some(inner[..end].to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_parse_from_fenced_reply() {
        let reply = "Here are the regions:\n```json\n[
            {\"label\": \"toolbar\", \"box\": [0, 0, 800, 40]},
            {\"label\": \"\", \"box\": [1, 2, 3, 4]},
            {\"label\": \"bad box\", \"box\": [1, 2, 3]},
            {\"label\": \"main text\", \"box\": [10.5, 60, 780, 500]}
        ]\n```";
        let regions = extract_regions(reply);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].label, "toolbar");
        assert_eq!(regions[1].r#box, [10.5, 60.0, 780.0, 500.0]);
    }

    #[test]
    fn regions_empty_on_garbage() {
        assert!(extract_regions("no json here").is_empty());
        assert!(extract_regions("] backwards [").is_empty());
        assert!(extract_regions("{\"not\": \"an array\"}").is_empty());
    }

    #[test]
    fn object_extraction() {
        let reply = "Sure: {\"found\": true, \"box\": [1, 2, 3, 4], \"reason\": \"\"} done";
        let obj = extract_object(reply).unwrap();
        assert_eq!(obj.get("found"), Some(&serde_json::Value::Bool(true)));
        assert!(extract_object("nothing").is_none());
    }

    #[test]
    fn codex_model_line_scan() {
        assert_eq!(
            codex_model_from_config("# comment\nmodel = \"gpt-5.2\"\n[profiles.x]\nmodel = \"other\"\n"),
            Some("gpt-5.2".to_string())
        );
        assert_eq!(
            codex_model_from_config("[table]\nmodel = \"below section\"\n"),
            None,
            "top-level scan stops at the first table"
        );
        assert_eq!(codex_model_from_config("model = 'single'\n"), Some("single".to_string()));
        assert_eq!(codex_model_from_config(""), None);
    }
}
