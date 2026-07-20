//! Fuzzy subsequence search — an exact port of the Python
//! `helpers/search_utility.py` (scores must stay bit-identical; the unit tests
//! below assert ground-truth values captured from the Python implementation).

/// Check whether `search` is a subsequence of `target`.
///
/// Returns `Some(score)` on match (lower is better), `None` otherwise.
/// An empty search string matches everything with score 0.
///
/// Scoring:
/// - spread penalty: distance between first and last matched positions
/// - word-boundary bonus: −10 for a match at position 0, −5 after ` -_./\`
/// - early-match bonus: + first_position × 0.5
pub fn subsequence_match(search: &str, target: &str) -> Option<f64> {
    if search.is_empty() {
        return Some(0.0);
    }

    // Spaces are stripped from the search string for more flexible matching.
    let search_chars: Vec<char> = search
        .to_lowercase()
        .chars()
        .filter(|&c| c != ' ')
        .collect();
    let target_chars: Vec<char> = target.to_lowercase().chars().collect();

    let mut match_positions = Vec::with_capacity(search_chars.len());
    let mut search_idx = 0;
    for (target_idx, &tc) in target_chars.iter().enumerate() {
        if search_idx >= search_chars.len() {
            break;
        }
        if search_chars[search_idx] == tc {
            match_positions.push(target_idx);
            search_idx += 1;
        }
    }

    if search_idx < search_chars.len() {
        return None;
    }

    let mut score = 0.0_f64;

    if match_positions.len() > 1 {
        let spread = match_positions[match_positions.len() - 1] - match_positions[0];
        score += spread as f64;
    }

    for &pos in &match_positions {
        if pos == 0 {
            score -= 10.0;
        } else if matches!(target_chars[pos - 1], ' ' | '-' | '_' | '.' | '/' | '\\') {
            score -= 5.0;
        }
    }

    if let Some(&first) = match_positions.first() {
        score += first as f64 * 0.5;
    }

    Some(score)
}

/// Filter and rank `items` by subsequence match on `key(item)`.
/// An empty search returns the items unchanged (original order, no filtering).
/// Ties keep insertion order (stable sort), matching Python's `list.sort`.
pub fn fuzzy_search<T, S: AsRef<str>>(
    search: &str,
    items: Vec<T>,
    key: impl Fn(&T) -> S,
) -> Vec<T> {
    if search.is_empty() {
        return items;
    }

    let mut matches: Vec<(f64, T)> = items
        .into_iter()
        .filter_map(|item| subsequence_match(search, key(&item).as_ref()).map(|s| (s, item)))
        .collect();
    matches.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    matches.into_iter().map(|(_, item)| item).collect()
}

/// Case-insensitive exact match on an item's shortcut. Returns the index of
/// the first matching item; takes priority over fuzzy results in the UI.
pub fn exact_shortcut_match<T, S: AsRef<str>>(
    search: &str,
    items: &[T],
    shortcut: impl Fn(&T) -> S,
) -> Option<usize> {
    let search_lower = search.to_lowercase();
    items
        .iter()
        .position(|item| shortcut(item).as_ref().to_lowercase() == search_lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ground-truth scores captured by running the Python implementation
    // (helpers/search_utility.py) on 2026-07-20.
    #[test]
    fn scores_match_python_implementation() {
        let cases: &[(&str, &str, Option<f64>)] = &[
            ("gwe", "google website", Some(-7.0)),
            ("g w", "google website", Some(-8.0)),
            ("goog", "google website", Some(-7.0)),
            ("web", "google website", Some(0.5)),
            ("xyz", "google website", None),
            ("", "google website", Some(0.0)),
            ("gw", "visual studio code", None),
            ("vsc", "visual studio code", Some(-1.0)),
            ("note", "notepad", Some(-7.0)),
            ("o", "google website", Some(0.5)),
            ("e", "google website", Some(2.5)),
            ("gw", "google website", Some(-8.0)),
            ("s", "visual studio code", Some(1.0)),
            ("code", "visual studio code", Some(5.0)),
            ("c ode", "visual studio code", Some(5.0)),
        ];
        for &(search, target, expected) in cases {
            let got = subsequence_match(search, target);
            assert_eq!(got, expected, "search={search:?} target={target:?}");
        }
    }

    fn sample_names() -> Vec<&'static str> {
        vec!["google website", "github repo", "notepad", "visual studio code"]
    }

    // Result orderings captured from the Python implementation on the same
    // sample list (test_search.py's printed "expected" values are unasserted
    // and partly wrong; these are the real outputs).
    #[test]
    fn fuzzy_ordering_matches_python_implementation() {
        let cases: &[(&str, &[&str])] = &[
            ("gwe", &["google website"]),
            ("g w", &["google website"]),
            ("gh", &["github repo"]),
            ("vsc", &["visual studio code"]),
            ("note", &["notepad"]),
            ("o", &["google website", "notepad", "github repo", "visual studio code"]),
            ("e", &["notepad", "google website", "github repo", "visual studio code"]),
            ("t", &["github repo", "notepad", "visual studio code", "google website"]),
        ];
        for &(search, expected) in cases {
            let got = fuzzy_search(search, sample_names(), |n| *n);
            assert_eq!(got, expected, "search={search:?}");
        }
    }

    #[test]
    fn empty_search_returns_all_items_in_original_order() {
        let got = fuzzy_search("", sample_names(), |n| *n);
        assert_eq!(got, sample_names());
    }

    #[test]
    fn ties_keep_insertion_order() {
        let items = vec!["abc one", "abc two", "abc three"];
        let got = fuzzy_search("abc", items.clone(), |n| *n);
        assert_eq!(got, items);
    }

    #[test]
    fn exact_shortcut_matching() {
        struct Cmd {
            name: &'static str,
            shortcut: &'static str,
        }
        let cmds = [
            Cmd { name: "google website", shortcut: "gw" },
            Cmd { name: "github repo", shortcut: "gh" },
        ];
        let hit = exact_shortcut_match("gw", &cmds, |c| c.shortcut);
        assert_eq!(hit.map(|i| cmds[i].name), Some("google website"));
        let hit = exact_shortcut_match("GH", &cmds, |c| c.shortcut);
        assert_eq!(hit.map(|i| cmds[i].name), Some("github repo"));
        assert_eq!(exact_shortcut_match("xyz", &cmds, |c| c.shortcut), None);
    }
}
