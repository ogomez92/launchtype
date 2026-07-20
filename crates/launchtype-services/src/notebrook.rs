//! Minimal Notebrook API client — port of `services/notebrook_service.py`
//! (which itself mirrors the Rust `notebroocli`). Single auth mechanism:
//! the raw token in an `authorization` header (no "Bearer" prefix).

use std::time::Duration;

use launchtype_core::i18n::tr;

const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct NotebrookError {
    pub message: String,
    /// True on HTTP 401 so callers can forget stored credentials and re-prompt.
    pub unauthorized: bool,
}

impl NotebrookError {
    fn new(message: String) -> Self {
        NotebrookError { message, unauthorized: false }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(TIMEOUT).build()
}

fn base_url(url: &str) -> &str {
    url.trim_end_matches('/')
}

/// Perform a request, returning the decoded JSON body (or None for an empty
/// or non-JSON body), with failures mapped to readable reasons.
fn request(
    method: &str,
    url: &str,
    token: &str,
    payload: Option<serde_json::Value>,
) -> Result<Option<serde_json::Value>, NotebrookError> {
    let mut req = agent().request(method, url).set("authorization", token);
    let result = match payload {
        Some(body) => {
            req = req.set("content-type", "application/json");
            req.send_string(&body.to_string())
        }
        None => req.call(),
    };
    match result {
        Ok(response) => {
            let body = response.into_string().map_err(|e| {
                NotebrookError::new(tr("Unexpected error: {}").replacen("{}", &e.to_string(), 1))
            })?;
            let body = body.trim();
            if body.is_empty() {
                return Ok(None);
            }
            Ok(serde_json::from_str(body).ok())
        }
        Err(ureq::Error::Status(401, _)) => Err(NotebrookError {
            message: tr("Unauthorized (401): the token was rejected."),
            unauthorized: true,
        }),
        Err(ureq::Error::Status(404, _)) => {
            Err(NotebrookError::new(tr("Not found (404): check the server URL is correct.")))
        }
        Err(ureq::Error::Status(code, _)) => Err(NotebrookError::new(
            tr("Server returned an unexpected status code: {}").replacen("{}", &code.to_string(), 1),
        )),
        Err(ureq::Error::Transport(t)) => Err(NotebrookError::new(
            tr("Network error: {}").replacen("{}", &t.to_string(), 1),
        )),
    }
}

/// Validate credentials against the /check-token endpoint.
pub fn check_token(url: &str, token: &str) -> Result<(), NotebrookError> {
    request("GET", &format!("{}/check-token", base_url(url)), token, None)?;
    Ok(())
}

pub fn get_channels(url: &str, token: &str) -> Result<Vec<serde_json::Value>, NotebrookError> {
    let body = request("GET", &format!("{}/channels/", base_url(url)), token, None)?;
    Ok(body
        .and_then(|b| b.get("channels").and_then(|c| c.as_array()).cloned())
        .unwrap_or_default())
}

pub fn create_channel(
    url: &str,
    token: &str,
    name: &str,
) -> Result<serde_json::Value, NotebrookError> {
    let body = request(
        "POST",
        &format!("{}/channels/", base_url(url)),
        token,
        Some(serde_json::json!({"name": name})),
    )?;
    match body {
        Some(channel) if channel.get("id").is_some() => Ok(channel),
        _ => Err(NotebrookError::new(tr("The server did not return the created channel."))),
    }
}

pub fn send_message(
    url: &str,
    token: &str,
    channel_id: &serde_json::Value,
    content: &str,
) -> Result<(), NotebrookError> {
    let id = match channel_id {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    request(
        "POST",
        &format!("{}/channels/{}/messages/", base_url(url), id),
        token,
        Some(serde_json::json!({"content": content})),
    )?;
    Ok(())
}

/// Send `content` to `channel_name`, creating the channel if missing.
pub fn send_note(
    url: &str,
    token: &str,
    channel_name: &str,
    content: &str,
) -> Result<(), NotebrookError> {
    let channels = get_channels(url, token)?;
    let channel = channels
        .into_iter()
        .find(|c| c.get("name").and_then(|n| n.as_str()) == Some(channel_name));
    let channel = match channel {
        Some(c) => c,
        None => create_channel(url, token, channel_name)?,
    };
    let id = channel
        .get("id")
        .cloned()
        .ok_or_else(|| NotebrookError::new(tr("The server did not return the created channel.")))?;
    send_message(url, token, &id, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_strips_trailing_slashes() {
        assert_eq!(base_url("https://x.example/"), "https://x.example");
        assert_eq!(base_url("https://x.example//"), "https://x.example");
        assert_eq!(base_url("https://x.example"), "https://x.example");
    }
}
