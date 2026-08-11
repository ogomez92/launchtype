//! Clipboard history logic — pure port of `services/clipboard_history.py`
//! (dedupe, front-insert, 50-item cap). The 100ms polling thread lives in
//! `launchtype-services`; persistence is a plain JSON array of strings.

pub const MAX_ITEMS: usize = 50;

/// How many secrets are remembered as unrecordable at once. A handful covers
/// looking several vault entries up in one sitting; the oldest is forgotten
/// after that, by which time it is long off the clipboard.
const MAX_SUPPRESSED: usize = 8;

#[derive(Default)]
pub struct ClipboardHistory {
    items: Vec<String>,
    last_value: Option<String>,
    /// SHA-256 of values that must never be recorded — vault secrets. Stored
    /// as digests rather than text so the history holds no copy of a password
    /// even in memory, and kept apart from `last_value` because that one is
    /// overwritten by the very next thing copied, which would let the secret
    /// back in on the tick after.
    suppressed: Vec<[u8; 32]>,
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
        ClipboardHistory { items, last_value: None, suppressed: Vec::new() }
    }

    /// Refuse to ever record `value`, whatever the poller sees later.
    ///
    /// Called *before* a vault secret is put on the clipboard: the poller runs
    /// on its own thread and would otherwise write the password into
    /// `clipboard_history.json`, which is the one place a vault secret must
    /// never end up.
    pub fn suppress(&mut self, value: &str) {
        let digest = fingerprint(value);
        if self.suppressed.contains(&digest) {
            return;
        }
        if self.suppressed.len() == MAX_SUPPRESSED {
            self.suppressed.remove(0);
        }
        self.suppressed.push(digest);
        // It may already be in the history from an earlier run or an earlier
        // copy of the same secret by other means.
        self.items.retain(|item| fingerprint(item) != digest);
    }

    fn is_suppressed(&self, value: &str) -> bool {
        !self.suppressed.is_empty() && self.suppressed.contains(&fingerprint(value))
    }

    /// Feed one clipboard poll result. Returns `true` when the history
    /// changed (the caller persists it). Empty values, repeats of the
    /// last-seen value and suppressed secrets are ignored.
    pub fn observe(&mut self, value: &str) -> bool {
        if value.is_empty() || self.last_value.as_deref() == Some(value) {
            return false;
        }
        if self.is_suppressed(value) {
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

/// SHA-256 of a clipboard value: a way to recognise a secret later without
/// keeping a copy of it around. Used by the suppression list above, and by the
/// delayed "clear the clipboard" that has to check whether what it is about to
/// wipe is still the secret it copied.
pub fn fingerprint(value: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(value.as_bytes()).as_slice().try_into().expect("SHA-256 is 32 bytes")
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

    /// The one guarantee vault mode makes about the clipboard: whatever the
    /// poller sees, the secret never reaches `clipboard_history.json`.
    #[test]
    fn a_suppressed_secret_is_never_recorded() {
        let mut h = ClipboardHistory::default();
        h.suppress("hunter2");
        assert!(!h.observe("hunter2"));
        assert!(h.items().is_empty());

        // Copying something else must not lift the suppression: `last_value`
        // alone would, and the very next poll would record the password.
        assert!(h.observe("ordinary text"));
        assert!(!h.observe("hunter2"));
        assert_eq!(h.items(), ["ordinary text"]);
    }

    #[test]
    fn suppressing_also_removes_a_secret_already_in_the_history() {
        let mut h = ClipboardHistory::default();
        h.observe("hunter2");
        h.observe("ordinary text");
        h.suppress("hunter2");
        assert_eq!(h.items(), ["ordinary text"]);
    }

    #[test]
    fn only_the_last_few_secrets_stay_suppressed() {
        let mut h = ClipboardHistory::default();
        for i in 0..MAX_SUPPRESSED + 1 {
            h.suppress(&format!("secret {i}"));
        }
        // The oldest fell out; everything after it is still refused.
        assert!(h.observe("secret 0"));
        for i in 1..MAX_SUPPRESSED + 1 {
            assert!(!h.observe(&format!("secret {i}")), "secret {i} was recorded");
        }
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
