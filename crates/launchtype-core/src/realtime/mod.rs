//! Realtime data mode ("+") — pure parsing/formatting port of
//! `services/realtime_service.py`. Every function here turns already-fetched
//! bytes (HTTP bodies, command output, local files) into the exact speakable
//! sentence the Python app produces; the HTTP/subprocess I/O lives in
//! `launchtype-services`.

pub mod history;
pub mod market;
pub mod number;
pub mod rss;
pub mod temperatures;
pub mod usage;
pub mod weather;

use crate::i18n::tr;

/// Network timeout in seconds for every request (Python `TIMEOUT`).
pub const TIMEOUT_SECONDS: u64 = 15;

/// Yahoo Finance rejects requests without a browser-looking user agent.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) launchtype";

/// A realtime data source cannot be fetched or parsed. Displays the localized
/// user-facing message, exactly like the Python `RealtimeError`; `code()`
/// carries the HTTP status when the failure was an HTTP error, so callers can
/// give a friendlier message for specific statuses (401 handling).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RealtimeError {
    #[error("{}", tr("Server returned an unexpected status code: {}").replacen("{}", &.0.to_string(), 1))]
    HttpStatus(u16),
    #[error("{}", tr("Network error: {}").replacen("{}", .0.as_str(), 1))]
    Network(String),
    #[error("{}", tr("Unexpected error: {}").replacen("{}", .0.as_str(), 1))]
    Unexpected(String),
    #[error("{}", tr("The server returned data that could not be understood."))]
    NotUnderstood,
    #[error("{}", tr("The news feed contained no headlines."))]
    NoHeadlines,
    #[error("{}", tr("Unknown realtime item."))]
    UnknownItem,
    #[error("{}", tr("Claude Code credentials not found, log in to Claude Code first."))]
    ClaudeCredentialsMissing,
    #[error("{}", tr("Claude Code session expired, open Claude Code to log in again."))]
    ClaudeSessionExpired,
    #[error("{}", tr("Codex credentials not found, log in to the Codex CLI first."))]
    CodexCredentialsMissing,
    #[error("{}", tr("Codex session expired, run Codex to log in again."))]
    CodexSessionExpired,
    #[error("{}", tr("No temperature, fan or GPU data is available on this computer."))]
    NoSensorData,
}

impl RealtimeError {
    /// The HTTP status code when the failure was an HTTP error (Python's
    /// `RealtimeError.code`).
    pub fn code(&self) -> Option<u16> {
        match self {
            RealtimeError::HttpStatus(code) => Some(*code),
            _ => None,
        }
    }

    /// The localized user-facing message (what the Python exception carries).
    pub fn message(&self) -> String {
        self.to_string()
    }
}

/// One entry of the "+" mode list (Python `get_realtime_items`): `key` is the
/// fetcher key and doubles as the item id; the type is always `"realtime"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealtimeItem {
    /// Localized display name.
    pub name: String,
    pub shortcut: &'static str,
    pub key: &'static str,
    /// Same value as `key` (mirrors the Python dict's `id`).
    pub id: &'static str,
    /// Always `"realtime"`.
    pub item_type: &'static str,
}

/// The list of realtime data items for the UI, in Python's order.
pub fn realtime_items() -> Vec<RealtimeItem> {
    let definitions: [(&'static str, String, &'static str); 14] = [
        ("bitcoin", tr("bitcoin price in euros"), "btc"),
        ("ethereum", tr("ethereum price in euros"), "eth"),
        ("eur_usd", tr("1000 euros in us dollars"), "usd"),
        ("brent", tr("brent crude oil price"), "oil"),
        ("gold", tr("gold price"), "gold"),
        ("ibex", tr("ibex 35 stock index"), "ibex"),
        ("weather", tr("weather at my location"), "w"),
        ("elpais", tr("el país news headlines"), "news"),
        ("catalunya", tr("catalunya news headlines"), "cat"),
        ("vilaweb", tr("vilaweb news in catalan"), "vila"),
        ("bbc", tr("bbc world news headlines"), "bbc"),
        ("claude", tr("claude usage limits"), "cc"),
        ("openai", tr("openai codex usage limits"), "co"),
        ("temperatures", tr("computer temperatures, fans and gpu"), "t"),
    ];
    definitions
        .into_iter()
        .map(|(key, name, shortcut)| RealtimeItem {
            name,
            shortcut,
            key,
            id: key,
            item_type: "realtime",
        })
        .collect()
}

/// Parse a response body as JSON; failures map to the same "could not be
/// understood" error the Python `_get_json` raises.
pub(crate) fn parse_json_body(body: &str) -> Result<serde_json::Value, RealtimeError> {
    serde_json::from_str(body).map_err(|_| RealtimeError::NotUnderstood)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_messages_match_python() {
        assert_eq!(
            RealtimeError::HttpStatus(500).to_string(),
            "Server returned an unexpected status code: 500"
        );
        assert_eq!(
            RealtimeError::Network("timed out".to_string()).to_string(),
            "Network error: timed out"
        );
        assert_eq!(
            RealtimeError::Unexpected("boom".to_string()).to_string(),
            "Unexpected error: boom"
        );
        assert_eq!(
            RealtimeError::NotUnderstood.to_string(),
            "The server returned data that could not be understood."
        );
        assert_eq!(
            RealtimeError::NoHeadlines.to_string(),
            "The news feed contained no headlines."
        );
        assert_eq!(RealtimeError::UnknownItem.to_string(), "Unknown realtime item.");
        assert_eq!(
            RealtimeError::ClaudeCredentialsMissing.to_string(),
            "Claude Code credentials not found, log in to Claude Code first."
        );
        assert_eq!(
            RealtimeError::ClaudeSessionExpired.to_string(),
            "Claude Code session expired, open Claude Code to log in again."
        );
        assert_eq!(
            RealtimeError::CodexCredentialsMissing.to_string(),
            "Codex credentials not found, log in to the Codex CLI first."
        );
        assert_eq!(
            RealtimeError::CodexSessionExpired.to_string(),
            "Codex session expired, run Codex to log in again."
        );
        assert_eq!(
            RealtimeError::NoSensorData.to_string(),
            "No temperature, fan or GPU data is available on this computer."
        );
    }

    #[test]
    fn http_code_is_exposed() {
        assert_eq!(RealtimeError::HttpStatus(401).code(), Some(401));
        assert_eq!(RealtimeError::NotUnderstood.code(), None);
    }

    #[test]
    fn items_match_python_definitions() {
        let items = realtime_items();
        assert_eq!(items.len(), 14);
        assert_eq!(items[0].key, "bitcoin");
        assert_eq!(items[0].name, "bitcoin price in euros");
        assert_eq!(items[0].shortcut, "btc");
        assert_eq!(items[0].id, "bitcoin");
        assert_eq!(items[0].item_type, "realtime");
        assert_eq!(items[6].key, "weather");
        assert_eq!(items[6].shortcut, "w");
        assert_eq!(items[13].key, "temperatures");
        assert_eq!(items[13].name, "computer temperatures, fans and gpu");
        assert!(items.iter().all(|item| item.id == item.key));
    }
}
