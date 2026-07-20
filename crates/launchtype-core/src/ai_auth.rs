//! Credential parsing for the AI/usage features (pure parts; the HTTP calls
//! live in `launchtype-services`).
//!
//! - Claude: OAuth access token from `~/.claude/.credentials.json`
//! - Codex: `~/.codex/auth.json` tokens with hourly-expiring JWTs. OpenAI
//!   rotates refresh tokens, so a refresh response must be written back
//!   (skipping the write-back would invalidate the stored refresh token).

use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Extract the Claude Code OAuth access token from the parsed
/// `.credentials.json` document.
pub fn claude_access_token(credentials: &serde_json::Value) -> Option<String> {
    let token = credentials.get("claudeAiOauth")?.get("accessToken")?.as_str()?;
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// `~/.codex/auth.json`. Unknown keys are preserved for the write-back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexAuth {
    pub tokens: CodexTokens,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refresh: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexTokens {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Parse auth.json contents; `None` when unreadable or missing tokens
/// (callers surface "log in to the Codex CLI first").
pub fn parse_codex_auth(text: &str) -> Option<CodexAuth> {
    let auth: CodexAuth = serde_json::from_str(text).ok()?;
    if auth.tokens.access_token.is_empty() || auth.tokens.refresh_token.is_empty() {
        return None;
    }
    Some(auth)
}

/// True when the JWT's `exp` claim is within `margin_seconds` of
/// `now_epoch_seconds` (or the token is unreadable).
pub fn jwt_is_expired(token: &str, now_epoch_seconds: f64, margin_seconds: f64) -> bool {
    let Some(payload) = token.split('.').nth(1) else {
        return true;
    };
    let decoded = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload) {
        Ok(bytes) => bytes,
        Err(_) => return true,
    };
    let claims: serde_json::Value = match serde_json::from_slice(&decoded) {
        Ok(v) => v,
        Err(_) => return true,
    };
    match claims.get("exp").and_then(|e| e.as_f64()) {
        Some(exp) => exp <= now_epoch_seconds + margin_seconds,
        None => true,
    }
}

/// The refresh request body sent to auth.openai.com/oauth/token.
pub fn codex_refresh_request_body(client_id: &str, refresh_token: &str) -> serde_json::Value {
    serde_json::json!({
        "client_id": client_id,
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "scope": "openid profile email",
    })
}

/// Fold a refresh response into `auth` (rotated tokens + `last_refresh`
/// stamp), mirroring the Python `_refresh_codex_tokens` write-back.
/// Returns `false` when the response carries no access_token.
pub fn apply_codex_refresh(
    auth: &mut CodexAuth,
    response: &serde_json::Value,
    now_utc: DateTime<Utc>,
) -> bool {
    let Some(access) = response.get("access_token").and_then(|t| t.as_str()) else {
        return false;
    };
    auth.tokens.access_token = access.to_string();
    if let Some(id_token) = response.get("id_token").and_then(|t| t.as_str()) {
        if !id_token.is_empty() {
            auth.tokens.id_token = Some(id_token.to_string());
        }
    }
    if let Some(refresh) = response.get("refresh_token").and_then(|t| t.as_str()) {
        if !refresh.is_empty() {
            auth.tokens.refresh_token = refresh.to_string();
        }
    }
    // Python: datetime.now(utc).isoformat(timespec="microseconds") + Z
    auth.last_refresh = Some(now_utc.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string());
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_jwt(exp: f64) -> String {
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#);
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(format!(r#"{{"exp": {exp}}}"#));
        format!("{header}.{payload}.sig")
    }

    #[test]
    fn claude_token_extraction() {
        let creds = serde_json::json!({"claudeAiOauth": {"accessToken": "tok-123"}});
        assert_eq!(claude_access_token(&creds), Some("tok-123".to_string()));
        assert_eq!(claude_access_token(&serde_json::json!({})), None);
        let empty = serde_json::json!({"claudeAiOauth": {"accessToken": ""}});
        assert_eq!(claude_access_token(&empty), None);
    }

    #[test]
    fn jwt_expiry_check() {
        let now = 1_800_000_000.0;
        assert!(!jwt_is_expired(&make_jwt(now + 3600.0), now, 60.0));
        assert!(jwt_is_expired(&make_jwt(now + 30.0), now, 60.0), "inside margin");
        assert!(jwt_is_expired(&make_jwt(now - 10.0), now, 60.0));
        assert!(jwt_is_expired("garbage", now, 60.0));
        assert!(jwt_is_expired("a.!!!.c", now, 60.0));
    }

    #[test]
    fn codex_auth_round_trip_preserves_unknown_keys() {
        let text = r#"{"OPENAI_API_KEY": null, "tokens": {"id_token": "idt", "access_token": "at", "refresh_token": "rt", "account_id": "acc-1"}, "last_refresh": "2026-07-19T10:00:00.000000Z"}"#;
        let auth = parse_codex_auth(text).unwrap();
        assert_eq!(auth.tokens.access_token, "at");
        assert_eq!(auth.tokens.extra.get("account_id").unwrap(), "acc-1");
        assert!(auth.extra.contains_key("OPENAI_API_KEY"));

        let missing = r#"{"tokens": {"access_token": "", "refresh_token": "rt"}}"#;
        assert!(parse_codex_auth(missing).is_none());
    }

    #[test]
    fn refresh_rotates_tokens_and_stamps_time() {
        let mut auth = parse_codex_auth(
            r#"{"tokens": {"access_token": "old-at", "refresh_token": "old-rt"}}"#,
        )
        .unwrap();
        let response = serde_json::json!({
            "access_token": "new-at",
            "refresh_token": "new-rt",
            "id_token": "new-idt",
        });
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 30, 45).unwrap()
            + chrono::Duration::microseconds(123456);
        assert!(apply_codex_refresh(&mut auth, &response, now));
        assert_eq!(auth.tokens.access_token, "new-at");
        assert_eq!(auth.tokens.refresh_token, "new-rt");
        assert_eq!(auth.tokens.id_token.as_deref(), Some("new-idt"));
        assert_eq!(auth.last_refresh.as_deref(), Some("2026-07-20T12:30:45.123456Z"));

        // Response without rotation keeps the old refresh token.
        let response = serde_json::json!({"access_token": "newer-at"});
        assert!(apply_codex_refresh(&mut auth, &response, now));
        assert_eq!(auth.tokens.refresh_token, "new-rt");

        assert!(!apply_codex_refresh(&mut auth, &serde_json::json!({}), now));
    }
}
