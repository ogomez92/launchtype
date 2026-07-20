//! Claude vision via the user's Claude Code subscription OAuth token
//! (`~/.claude/.credentials.json`). The token is only accepted when the
//! request presents the fixed Claude Code system identity.

use base64::Engine;
use launchtype_core::ai_auth::claude_access_token;
use launchtype_core::i18n::tr;

use super::AiError;
use crate::USER_AGENT;

// This identity is what makes the subscription OAuth token usable: it is a
// protocol string, NOT user-facing text — never wrap it in tr() or change it.
const CLAUDE_CODE_IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";
const CLAUDE_URL: &str = "https://api.anthropic.com/v1/messages";
const MAX_TOKENS: u32 = 1024;

pub(super) const AI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

fn read_claude_token() -> Result<String, AiError> {
    let not_found =
        || AiError(tr("Claude Code credentials not found, log in to Claude Code first."));
    let path = dirs::home_dir().ok_or_else(not_found)?.join(".claude").join(".credentials.json");
    let text = std::fs::read_to_string(path).map_err(|_| not_found())?;
    let credentials: serde_json::Value = serde_json::from_str(&text).map_err(|_| not_found())?;
    claude_access_token(&credentials).ok_or_else(not_found)
}

pub fn describe_with_claude(
    image_bytes: &[u8],
    prompt: &str,
    model: &str,
) -> Result<String, AiError> {
    let token = read_claude_token()?;
    let encoded_image = base64::engine::general_purpose::STANDARD.encode(image_bytes);
    let body = serde_json::json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "system": [{"type": "text", "text": CLAUDE_CODE_IDENTITY}],
        "messages": [{
            "role": "user",
            "content": [
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/jpeg",
                        "data": encoded_image,
                    },
                },
                {"type": "text", "text": prompt},
            ],
        }],
    });

    let agent = ureq::AgentBuilder::new().timeout(AI_TIMEOUT).build();
    let response = agent
        .post(CLAUDE_URL)
        .set("Authorization", &format!("Bearer {token}"))
        .set("anthropic-version", "2023-06-01")
        .set("anthropic-beta", "oauth-2025-04-20")
        .set("content-type", "application/json")
        .set("User-Agent", USER_AGENT)
        .send_string(&body.to_string());

    let response = match response {
        Ok(r) => r,
        Err(ureq::Error::Status(401, _)) => {
            return Err(AiError(tr("Claude Code session expired, open Claude Code to log in again.")))
        }
        Err(ureq::Error::Status(code, _)) => {
            return Err(AiError(
                tr("Server returned an unexpected status code: {}").replacen("{}", &code.to_string(), 1),
            ))
        }
        Err(ureq::Error::Transport(t)) => {
            return Err(AiError(tr("Network error: {}").replacen("{}", &t.to_string(), 1)))
        }
    };

    let not_understood = || AiError(tr("The server returned data that could not be understood."));
    let data: serde_json::Value =
        serde_json::from_str(&response.into_string().map_err(|_| not_understood())?)
            .map_err(|_| not_understood())?;
    let text: String = data
        .get("content")
        .and_then(|c| c.as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .concat()
        })
        .unwrap_or_default()
        .trim()
        .to_string();
    if text.is_empty() {
        return Err(not_understood());
    }
    Ok(text)
}
