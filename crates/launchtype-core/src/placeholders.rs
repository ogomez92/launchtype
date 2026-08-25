//! `placeholders.json` — the placeholders the user writes for themselves.
//!
//! The built-in catalog answers questions about the *machine*
//! ([`crate::portable`]) and about the *clock* ([`crate::snippet_vars`]).
//! Neither knows the sentence you type forty times a week. This is the third
//! kind: a name and the text it stands for, written once and used everywhere
//! `{{name}}` is understood — in a snippet, and in a command's path or
//! arguments, because they share one placeholder syntax and there is no
//! reason a name should work in one and not the other.
//!
//! The text is inserted exactly as written, and placeholders *inside* it are
//! not expanded again. One pass is predictable and cannot loop; a name that
//! stands for itself would otherwise hang the app rather than paste a word.
//!
//! The file lives next to the snippets it is mostly written for, is a plain
//! JSON object, and is meant to be readable by whoever opens it:
//!
//! ```json
//! {
//!   "hi": "Hola, ¿qué tal?",
//!   "sig": "Un saludo,\nOscar"
//!}
//! ```

use std::collections::BTreeMap;

use crate::i18n::{format_args, tr, Arg};
use crate::portable::{VarValue, CLOSE, OPEN};

/// What the file is called, inside the snippets folder.
pub const FILE_NAME: &str = "placeholders.json";

/// The user's own placeholders, kept sorted by name so that the insert menu
/// and the file on disk both stay in a stable, findable order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Placeholders {
    /// `(name as the user wrote it, the text it stands for)`.
    entries: Vec<(String, String)>,
}

impl Placeholders {
    /// The text `name` stands for, whatever case it is written in.
    pub fn get(&self, name: &str) -> Option<&str> {
        let folded = fold(name);
        self.entries
            .iter()
            .find(|(other, _)| fold(other) == folded)
            .map(|(_, text)| text.as_str())
    }

    /// Define `name`, replacing any existing one. The new spelling wins: a
    /// user retyping `Hi` for `hi` has renamed it, not made a second one.
    pub fn set(&mut self, name: &str, text: &str) {
        let name = name.trim();
        self.remove(name);
        self.entries.push((name.to_string(), text.to_string()));
        self.entries.sort_by_key(|(name, _)| fold(name));
    }

    /// Forget `name`. True when there was one to forget.
    pub fn remove(&mut self, name: &str) -> bool {
        let folded = fold(name);
        let before = self.entries.len();
        self.entries.retain(|(other, _)| fold(other) != folded);
        self.entries.len() != before
    }

    /// Every placeholder, sorted by name.
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// These placeholders as [`crate::portable::Vars`] entries, so a command
    /// expands them at launch alongside `{{home}}` and `{{chrome}}`.
    ///
    /// [`VarValue::Text`] rather than `Path`: this is prose, and the separator
    /// normalisation a path gets would rewrite the slashes in a date.
    pub fn vars(&self) -> Vec<(String, VarValue)> {
        self.entries
            .iter()
            .map(|(name, text)| (fold(name), VarValue::Text(text.clone())))
            .collect()
    }

    /// Read the file's contents. Anything malformed yields an empty set rather
    /// than an error: this file is hand-editable, and a stray comma must cost
    /// the user their placeholders for as long as it takes to fix, not stop
    /// the app from starting.
    pub fn from_json(text: &str) -> Self {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return Placeholders::default();
        };
        let Some(object) = value.as_object() else {
            return Placeholders::default();
        };
        let mut placeholders = Placeholders::default();
        for (name, value) in object {
            // A non-string value is somebody's half-finished edit; skip it
            // rather than pasting `null` into their mail.
            if let Some(text) = value.as_str() {
                if !name.trim().is_empty() {
                    placeholders.set(name, text);
                }
            }
        }
        placeholders
    }

    /// The file's contents: a sorted, indented JSON object, because people
    /// open this one by hand.
    pub fn to_json(&self) -> String {
        let map: BTreeMap<&str, &str> =
            self.entries.iter().map(|(name, text)| (name.as_str(), text.as_str())).collect();
        serde_json::to_string_pretty(&map).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Why `name` cannot be used for a placeholder of one's own, or `None` when it
/// can. The message is meant to be shown to the user as it comes.
///
/// `reserved` is every name the app already answers itself — passed in rather
/// than looked up, because the catalog of those lives in `launchtype-services`
/// (it depends on what is installed) and this module stays pure.
pub fn name_error(name: &str, reserved: &[String]) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return Some(tr("Please give the variable a name."));
    }
    if name.contains(OPEN) || name.contains(CLOSE) || name.contains(['{', '}']) {
        // The braces are how the name is recognised; one inside it would end
        // the placeholder early and leave the rest of the name in the text.
        return Some(tr("A variable name cannot contain { or }."));
    }
    let folded = fold(name);
    if reserved.iter().any(|other| fold(other) == folded) {
        return Some(format_args(
            &tr("{name} is already a Launchtype variable. Please pick another name."),
            &[("name", Arg::Str(name))],
        ));
    }
    None
}

/// How two spellings of one name are compared. Not `eq_ignore_ascii_case`:
/// these are written in Spanish as often as in English, and `Ñ` has to fold
/// to `ñ`.
fn fold(name: &str) -> String {
    name.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hi() -> Placeholders {
        let mut placeholders = Placeholders::default();
        placeholders.set("hi", "Hola, ¿qué tal?");
        placeholders
    }

    #[test]
    fn a_name_stands_for_its_text_whatever_case_it_is_asked_for_in() {
        let placeholders = hi();
        assert_eq!(placeholders.get("hi"), Some("Hola, ¿qué tal?"));
        assert_eq!(placeholders.get("HI"), Some("Hola, ¿qué tal?"));
        assert_eq!(placeholders.get(" Hi "), Some("Hola, ¿qué tal?"));
        assert_eq!(placeholders.get("bye"), None);
    }

    /// Setting an existing name is how one is edited, so it must replace
    /// rather than accumulate — including when the case has changed.
    #[test]
    fn setting_an_existing_name_replaces_it() {
        let mut placeholders = hi();
        placeholders.set("HI", "Buenas");
        assert_eq!(placeholders.entries(), [("HI".to_string(), "Buenas".to_string())]);
        assert!(placeholders.remove("hi"));
        assert!(!placeholders.remove("hi"));
        assert!(placeholders.is_empty());
    }

    #[test]
    fn entries_are_sorted_by_name_so_the_menu_is_stable() {
        let mut placeholders = Placeholders::default();
        for name in ["zeta", "Álvaro", "hi", "bye"] {
            placeholders.set(name, "x");
        }
        let names: Vec<&str> = placeholders.entries().iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["bye", "hi", "zeta", "Álvaro"]);
    }

    /// Written to be read: the file is in the snippets folder precisely so
    /// that people open it.
    #[test]
    fn the_file_is_a_plain_sorted_json_object() {
        let mut placeholders = hi();
        placeholders.set("sig", "Un saludo,\nOscar");
        let json = placeholders.to_json();
        assert_eq!(json, "{\n  \"hi\": \"Hola, ¿qué tal?\",\n  \"sig\": \"Un saludo,\\nOscar\"\n}");
        assert_eq!(Placeholders::from_json(&json), placeholders, "round trip");
    }

    /// This file is hand-edited, so every way of getting it wrong has to end
    /// somewhere survivable.
    #[test]
    fn a_broken_file_costs_the_placeholders_and_nothing_else() {
        assert!(Placeholders::from_json("{,}").is_empty());
        assert!(Placeholders::from_json("").is_empty());
        assert!(Placeholders::from_json("[1, 2]").is_empty());
        // A half-finished entry is skipped; its neighbours still load.
        let mixed = Placeholders::from_json(r#"{"hi": "hola", "oops": null, "  ": "x"}"#);
        assert_eq!(mixed.entries(), [("hi".to_string(), "hola".to_string())]);
    }

    #[test]
    fn placeholders_expand_as_text_not_as_paths() {
        assert_eq!(
            hi().vars(),
            [("hi".to_string(), VarValue::Text("Hola, ¿qué tal?".to_string()))]
        );
    }

    #[test]
    fn a_name_may_not_be_empty_hold_braces_or_shadow_a_builtin() {
        let reserved = ["home".to_string(), "query".to_string()];
        assert!(name_error("informe", &reserved).is_none());
        assert!(name_error("fecha de solicitud", &reserved).is_none(), "spaces are fine");
        assert!(name_error("   ", &reserved).is_some());
        assert!(name_error("a{{b", &reserved).is_some());
        assert!(name_error("a}", &reserved).is_some());
        // Case-insensitively reserved, or `{{HOME}}` would still be the catalog's.
        assert!(name_error("Home", &reserved).is_some());
        assert!(name_error("query", &reserved).is_some());
    }
}
