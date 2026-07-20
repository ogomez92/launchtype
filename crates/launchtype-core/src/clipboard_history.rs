//! Clipboard history logic — pure port of `services/clipboard_history.py`
//! (dedupe, front-insert, 50-item cap). The 100ms polling thread lives in
//! `launchtype-services`; persistence is a plain JSON array of strings.

pub const MAX_ITEMS: usize = 50;

#[derive(Default)]
pub struct ClipboardHistory {
    items: Vec<String>,
    last_value: Option<String>,
}

impl ClipboardHistory {
    /// Non-string entries in a loaded file are filtered out, like Python.
    pub fn from_loaded(values: Vec<serde_json::Value>) -> Self {
        let items = values
            .into_iter()
            .filter_map(|v| match v {
                serde_json::Value::String(s) => Some(s),
                _ => None,
            })
            .collect();
        ClipboardHistory { items, last_value: None }
    }

    /// Feed one clipboard poll result. Returns `true` when the history
    /// changed (the caller persists it). Empty values and repeats of the
    /// last-seen value are ignored.
    pub fn observe(&mut self, value: &str) -> bool {
        if value.is_empty() || self.last_value.as_deref() == Some(value) {
            return false;
        }
        self.last_value = Some(value.to_string());
        self.items.retain(|item| item != value);
        self.items.insert(0, value.to_string());
        self.items.truncate(MAX_ITEMS);
        true
    }

    /// After the app itself writes the clipboard, forget the last value so
    /// the next poll re-records it at the front.
    pub fn forget_last_value(&mut self) {
        self.last_value = None;
    }

    /// Returns `true` when something was actually removed.
    pub fn delete_by_text(&mut self, text: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|item| item != text);
        self.items.len() != before
    }

    pub fn items(&self) -> &[String] {
        &self.items
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

/// Load history; missing or corrupt file yields an empty list.
pub fn load_history(path: &std::path::Path) -> ClipboardHistory {
    let values = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<serde_json::Value>>(&text).ok())
        .unwrap_or_default();
    ClipboardHistory::from_loaded(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_inserts_at_front_and_dedupes() {
        let mut h = ClipboardHistory::default();
        assert!(h.observe("one"));
        assert!(h.observe("two"));
        assert_eq!(h.items(), ["two", "one"]);

        // Re-copying an older value moves it to the front.
        assert!(h.observe("one"));
        assert_eq!(h.items(), ["one", "two"]);
    }

    #[test]
    fn repeats_and_empties_are_ignored() {
        let mut h = ClipboardHistory::default();
        assert!(h.observe("same"));
        assert!(!h.observe("same"), "same value twice in a row: no change");
        assert!(!h.observe(""), "empty clipboard: no change");
        assert_eq!(h.items().len(), 1);

        h.forget_last_value();
        assert!(h.observe("same"), "after forget, the same value re-records");
    }

    #[test]
    fn capped_at_50_items() {
        let mut h = ClipboardHistory::default();
        for i in 0..60 {
            h.observe(&format!("item {i}"));
        }
        assert_eq!(h.items().len(), MAX_ITEMS);
        assert_eq!(h.items()[0], "item 59");
        assert_eq!(h.items()[MAX_ITEMS - 1], "item 10");
    }

    #[test]
    fn loaded_file_filters_non_strings() {
        let values = serde_json::from_str::<Vec<serde_json::Value>>(
            r#"["keep", 42, null, {"x": 1}, "also keep"]"#,
        )
        .unwrap();
        let h = ClipboardHistory::from_loaded(values);
        assert_eq!(h.items(), ["keep", "also keep"]);
    }

    #[test]
    fn delete_by_text() {
        let mut h = ClipboardHistory::default();
        h.observe("a");
        h.observe("b");
        assert!(h.delete_by_text("a"));
        assert!(!h.delete_by_text("a"));
        assert_eq!(h.items(), ["b"]);
    }
}
