//! Claude vision via the user's Claude Code subscription OAuth token
//! (`~/.claude/.credentials.json`, or the login Keychain on macOS). The token
//! is only accepted when the request presents the fixed Claude Code system
//! identity.

use base64::Engine;
use launchtype_core::ai_auth::claude_access_token;
use launchtype_core::i18n::tr;

use super::AiError;
use crate::USER_AGENT;

// This identity is what makes the subscription OAuth token usable: it is a
// protocol string, NOT user-facing text — never wrap it in tr() or change it.
const CLAUDE_CODE_IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";
const CLAUDE_URL: &str = "https://api.anthropic.com/v1/messages";

/// Enough for a paragraph or two about a screenshot.
const DESCRIPTION_TOKENS: u32 = 1024;

/// Enough for a summary, a proofread page or a translation to come back whole.
/// A truncated answer is worse than a slow one here: the user pastes it.
pub const DOCUMENT_TOKENS: u32 = 8192;

pub(super) const AI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// A whole document read and answered takes longer than a screenshot looked
/// at, and the failure mode of too short a timeout here is losing work the
/// model had already done.
const DOCUMENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Long enough for Claude Code to start, refresh and answer one word; a hung
/// CLI must not keep the caller waiting forever.
const REFRESH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

fn read_claude_token() -> Result<String, AiError> {
    let not_found =
        || AiError(tr("Claude Code credentials not found, log in to Claude Code first."));
    claude_token_from_disk().ok_or_else(not_found)
}

/// The access token Claude Code keeps in `~/.claude/.credentials.json` (or,
/// on macOS, in the login Keychain), read fresh each time so a refresh by the
/// CLI is picked up.
pub fn claude_token_from_disk() -> Option<String> {
    let text = credentials_file().or_else(credentials_keychain)?;
    let credentials: serde_json::Value = serde_json::from_str(&text).ok()?;
    claude_access_token(&credentials)
}

fn credentials_file() -> Option<String> {
    let path = dirs::home_dir()?.join(".claude").join(".credentials.json");
    std::fs::read_to_string(path).ok()
}

/// On macOS Claude Code never writes `.credentials.json`: the same JSON goes
/// into a generic password item in the login Keychain instead. The first read
/// makes macOS ask the user to allow it.
#[cfg(target_os = "macos")]
fn credentials_keychain() -> Option<String> {
    // Absolute path, never a `PATH` lookup — see `program.rs`.
    let output = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[cfg(not(target_os = "macos"))]
fn credentials_keychain() -> Option<String> {
    None
}

/// Let Claude Code renew its own session. The access token lives a few hours
/// and the CLI swaps it for a new one (rotating the refresh token too) the
/// next time it runs; a 401 here nearly always means only that nobody has run
/// it lately, not that the user has to log in again. So run it once, headless,
/// with the cheapest possible request, and let it write the new token back to
/// `.credentials.json`.
///
/// Refreshing ourselves would mean holding Claude Code's rotating refresh
/// token and racing the CLI for it; asking the CLI keeps it the only writer.
///
/// Returns `true` when the CLI ran and exited cleanly; the caller re-reads the
/// token and retries to learn whether that fixed it.
pub fn refresh_claude_session() -> bool {
    let Some(claude) = find_claude() else {
        return false;
    };
    let mut command = std::process::Command::new(claude);
    command
        .args([
            "-p",
            "ok",
            "--model",
            "haiku",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--tools",
            "",
        ])
        // Any of these would make the CLI authenticate with them instead of
        // the stored login, and never touch the token that needs renewing.
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .env_remove("CLAUDE_CODE_OAUTH_TOKEN")
        // Away from the app folder, so no project CLAUDE.md or settings apply.
        .current_dir(std::env::temp_dir())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if started.elapsed() < REFRESH_TIMEOUT => {
                std::thread::sleep(std::time::Duration::from_millis(100))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// `claude` off `PATH`, or where the native installer puts it — the app may
/// have been started with a `PATH` from before Claude Code was installed.
fn find_claude() -> Option<std::path::PathBuf> {
    crate::program::on_path("claude").or_else(|| {
        let name = if cfg!(windows) { "claude.exe" } else { "claude" };
        let installed = dirs::home_dir()?.join(".local").join("bin").join(name);
        installed.is_file().then_some(installed)
    })
}

pub fn describe_with_claude(
    image_bytes: &[u8],
    prompt: &str,
    model: &str,
) -> Result<String, AiError> {
    let encoded_image = base64::engine::general_purpose::STANDARD.encode(image_bytes);
    let content = serde_json::json!([
        {
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": "image/jpeg",
                "data": encoded_image,
            },
        },
        {"type": "text", "text": prompt},
    ]);
    ask_claude(content, model, DESCRIPTION_TOKENS)
}

/// One turn of conversation with Claude, over whatever content blocks the
/// caller has built: an image for the screenshot flows, a document and a
/// question for path mode.
///
/// The subscription token is only honoured when the request presents the
/// Claude Code system identity above, which is why every call comes through
/// here rather than building its own request.
pub fn ask_claude(
    content: serde_json::Value,
    model: &str,
    max_tokens: u32,
) -> Result<String, AiError> {
    let body = serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": [{"type": "text", "text": CLAUDE_CODE_IDENTITY}],
        "messages": [{"role": "user", "content": content}],
    })
    .to_string();

    let timeout = if max_tokens > DESCRIPTION_TOKENS { DOCUMENT_TIMEOUT } else { AI_TIMEOUT };
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let send = |token: &str| {
        agent
            .post(CLAUDE_URL)
            .set("Authorization", &format!("Bearer {token}"))
            .set("anthropic-version", "2023-06-01")
            .set("anthropic-beta", "oauth-2025-04-20")
            .set("content-type", "application/json")
            .set("User-Agent", USER_AGENT)
            .send_string(&body)
    };

    let response = match send(&read_claude_token()?) {
        // Usually just a stale token: let Claude Code renew it, then try once more.
        Err(ureq::Error::Status(401, _)) if refresh_claude_session() => {
            send(&read_claude_token()?)
        }
        other => other,
    };

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
