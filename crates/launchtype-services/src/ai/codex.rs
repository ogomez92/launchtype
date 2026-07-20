//! OpenAI/Codex vision fallback: the Codex CLI's ChatGPT OAuth tokens from
//! `~/.codex/auth.json`, calling the streamed Responses endpoint the CLI
//! uses. Access tokens expire hourly and OpenAI rotates refresh tokens, so
//! refreshed tokens are written back to auth.json exactly like Codex does.

use std::io::BufRead;
use std::path::PathBuf;

use base64::Engine;
use launchtype_core::ai_auth::{
    apply_codex_refresh, codex_refresh_request_body, jwt_is_expired, parse_codex_auth, CodexAuth,
};
use launchtype_core::i18n::tr;
use launchtype_core::storage::atomic_write_json;

use super::claude::AI_TIMEOUT;
use super::parse::codex_model_from_config;
use super::AiError;
use crate::USER_AGENT;

const CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
/// Used only when the Codex config does not name a model.
const DEFAULT_CODEX_MODEL: &str = "gpt-5.1-codex";
const JWT_MARGIN_SECONDS: f64 = 60.0;

pub(super) fn load_codex_auth() -> Result<(PathBuf, CodexAuth), AiError> {
    let not_found = || AiError(tr("Codex credentials not found, log in to the Codex CLI first."));
    let path = dirs::home_dir().ok_or_else(not_found)?.join(".codex").join("auth.json");
    let text = std::fs::read_to_string(&path).map_err(|_| not_found())?;
    let auth = parse_codex_auth(&text).ok_or_else(not_found)?;
    Ok((path, auth))
}

fn now_epoch_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Refresh the access token and persist the rotated tokens. A failed
/// write-back is tolerated (the fetch proceeds with the in-memory token).
pub(super) fn refresh_codex_tokens(path: &PathBuf, auth: &mut CodexAuth) -> Result<(), AiError> {
    let expired = || AiError(tr("Codex session expired, run Codex to log in again."));
    let body = codex_refresh_request_body(CODEX_CLIENT_ID, &auth.tokens.refresh_token);
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(15)).build();
    let response = agent
        .post(CODEX_TOKEN_URL)
        .set("User-Agent", USER_AGENT)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|_| expired())?;
    let refreshed: serde_json::Value =
        serde_json::from_str(&response.into_string().map_err(|_| expired())?)
            .map_err(|_| expired())?;
    if !apply_codex_refresh(auth, &refreshed, chrono::Utc::now()) {
        return Err(expired());
    }
    let _ = atomic_write_json(path, auth, Some(2));
    Ok(())
}

fn codex_model() -> String {
    dirs::home_dir()
        .map(|home| home.join(".codex").join("config.toml"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| codex_model_from_config(&text))
        .unwrap_or_else(|| DEFAULT_CODEX_MODEL.to_string())
}

/// POST a Responses request and accumulate the streamed answer text.
fn codex_stream_text(auth: &CodexAuth, body: &str) -> Result<String, ureq::Error> {
    let agent = ureq::AgentBuilder::new().timeout(AI_TIMEOUT).build();
    let mut request = agent
        .post(CODEX_RESPONSES_URL)
        .set("Authorization", &format!("Bearer {}", auth.tokens.access_token))
        .set("Content-Type", "application/json")
        .set("OpenAI-Beta", "responses=experimental")
        .set("originator", "codex_cli_rs")
        .set("Accept", "text/event-stream")
        .set("User-Agent", USER_AGENT);
    if let Some(account_id) = auth.tokens.extra.get("account_id").and_then(|v| v.as_str()) {
        request = request.set("chatgpt-account-id", account_id);
    }
    let response = request.send_string(body)?;

    let reader = std::io::BufReader::new(response.into_reader());
    let mut parts = String::new();
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        let Some(payload) = line.strip_prefix("data:") else { continue };
        let payload = payload.trim();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(payload) else { continue };
        if event.get("type").and_then(|t| t.as_str()) == Some("response.output_text.delta") {
            if let Some(delta) = event.get("delta").and_then(|d| d.as_str()) {
                parts.push_str(delta);
            }
        }
    }
    Ok(parts.trim().to_string())
}

pub fn describe_with_openai(image_bytes: &[u8], prompt: &str) -> Result<String, AiError> {
    let (path, mut auth) = load_codex_auth()?;
    if jwt_is_expired(&auth.tokens.access_token, now_epoch_seconds(), JWT_MARGIN_SECONDS) {
        refresh_codex_tokens(&path, &mut auth)?;
    }

    let encoded_image = base64::engine::general_purpose::STANDARD.encode(image_bytes);
    let body = serde_json::json!({
        "model": codex_model(),
        "instructions": tr("You describe images for a blind user. Follow the user's request."),
        "input": [{
            "type": "message",
            "role": "user",
            "content": [
                {"type": "input_text", "text": prompt},
                {"type": "input_image", "image_url": format!("data:image/jpeg;base64,{encoded_image}")},
            ],
        }],
        "stream": true,
        "store": false,
    })
    .to_string();

    let unexpected = |code: u16| {
        AiError(tr("Server returned an unexpected status code: {}").replacen("{}", &code.to_string(), 1))
    };
    let text = match codex_stream_text(&auth, &body) {
        Ok(text) => text,
        // Token rejected despite not looking expired; refresh once and retry.
        Err(ureq::Error::Status(401, _)) => {
            refresh_codex_tokens(&path, &mut auth)?;
            match codex_stream_text(&auth, &body) {
                Ok(text) => text,
                Err(ureq::Error::Status(code, _)) => return Err(unexpected(code)),
                Err(ureq::Error::Transport(t)) => {
                    return Err(AiError(tr("Network error: {}").replacen("{}", &t.to_string(), 1)))
                }
            }
        }
        Err(ureq::Error::Status(code, _)) => return Err(unexpected(code)),
        Err(ureq::Error::Transport(t)) => {
            return Err(AiError(tr("Network error: {}").replacen("{}", &t.to_string(), 1)))
        }
    };

    if text.is_empty() {
        return Err(AiError(tr("The server returned data that could not be understood.")));
    }
    Ok(text)
}
