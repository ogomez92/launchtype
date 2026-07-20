//! Realtime data fetchers — the I/O half of `services/realtime_service.py`.
//! HTTP/subprocess results feed the pure sentence builders in
//! `launchtype_core::realtime`; every fetcher returns a ready-to-speak
//! sentence or a `RealtimeError` with the localized reason.

use std::time::Duration;

use base64::Engine;
use chrono::Local;
use launchtype_core::ai_auth::{claude_access_token, jwt_is_expired};
use launchtype_core::realtime::history::{HistoryStore, HISTORY_FILE};
use launchtype_core::realtime::market::{
    bitcoin_sentence, brent_sentence, coingecko_price_url, ethereum_sentence, eur_usd_sentence,
    gold_sentence, ibex_sentence, yahoo_chart_url, BRENT_SYMBOL, COINGECKO_BITCOIN_ID,
    COINGECKO_ETHEREUM_ID, FRANKFURTER_URL, GOLD_SYMBOL, IBEX_SYMBOL,
};
use launchtype_core::realtime::rss::{rss_headline_sentence, BBC, CATALUNYA, ELPAIS, VILAWEB};
use launchtype_core::realtime::temperatures::{
    collect_hwmonitor_sensors, parse_nvidia_smi, parse_windows_sensors, temperatures_sentence,
    HwSensor, HWMONITOR_TIMEOUT_SECONDS, HWMONITOR_URL, NVIDIA_SMI_ARGS, SENSORS_POWERSHELL,
};
use launchtype_core::realtime::usage::{
    claude_usage_sentence, openai_usage_sentence, CLAUDE_OAUTH_BETA, CLAUDE_USAGE_URL,
    CODEX_USAGE_URL,
};
use launchtype_core::realtime::weather::{
    open_meteo_url, parse_location, weather_sentence, IPINFO_URL,
};
use launchtype_core::realtime::{RealtimeError, TIMEOUT_SECONDS, USER_AGENT};

use crate::ai::{load_codex_auth_for_usage, refresh_codex_tokens_for_usage};

fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn http_get(url: &str, headers: &[(&str, &str)]) -> Result<String, RealtimeError> {
    http_get_with_timeout(url, headers, Duration::from_secs(TIMEOUT_SECONDS))
}

fn http_get_with_timeout(
    url: &str,
    headers: &[(&str, &str)],
    timeout: Duration,
) -> Result<String, RealtimeError> {
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let mut request = agent.get(url).set("User-Agent", USER_AGENT);
    for (name, value) in headers {
        request = request.set(name, value);
    }
    match request.call() {
        Ok(response) => response
            .into_string()
            .map_err(|e| RealtimeError::Unexpected(e.to_string())),
        Err(ureq::Error::Status(code, _)) => Err(RealtimeError::HttpStatus(code)),
        Err(ureq::Error::Transport(t)) => Err(RealtimeError::Network(t.to_string())),
    }
}

/// Fetch the realtime value for `key` and return a speakable sentence.
pub fn fetch_value(key: &str) -> Result<String, RealtimeError> {
    let history = HistoryStore::new(HISTORY_FILE);
    match key {
        "bitcoin" => {
            let body = http_get(&coingecko_price_url(COINGECKO_BITCOIN_ID), &[])?;
            bitcoin_sentence(&body, &history, now_epoch())
        }
        "ethereum" => {
            let body = http_get(&coingecko_price_url(COINGECKO_ETHEREUM_ID), &[])?;
            ethereum_sentence(&body, &history, now_epoch())
        }
        "eur_usd" => {
            let body = http_get(FRANKFURTER_URL, &[])?;
            eur_usd_sentence(&body, &history, now_epoch())
        }
        "brent" => {
            let body = http_get(&yahoo_chart_url(BRENT_SYMBOL), &[])?;
            brent_sentence(&body, &history, now_epoch())
        }
        "gold" => {
            let body = http_get(&yahoo_chart_url(GOLD_SYMBOL), &[])?;
            gold_sentence(&body, &history, now_epoch())
        }
        "ibex" => {
            let body = http_get(&yahoo_chart_url(IBEX_SYMBOL), &[])?;
            ibex_sentence(&body, &history, now_epoch())
        }
        "weather" => {
            let location_body = http_get(IPINFO_URL, &[]).ok();
            let (city, latitude, longitude) = parse_location(location_body.as_deref());
            let body = http_get(&open_meteo_url(latitude, longitude), &[])?;
            weather_sentence(&body, &city)
        }
        "elpais" => rss_headline_sentence(&http_get(ELPAIS.url, &[])?, ELPAIS.name),
        "catalunya" => rss_headline_sentence(&http_get(CATALUNYA.url, &[])?, CATALUNYA.name),
        "vilaweb" => rss_headline_sentence(&http_get(VILAWEB.url, &[])?, VILAWEB.name),
        "bbc" => rss_headline_sentence(&http_get(BBC.url, &[])?, BBC.name),
        "claude" => fetch_claude_usage(),
        "openai" => fetch_openai_usage(),
        "temperatures" => fetch_temperatures(),
        _ => Err(RealtimeError::UnknownItem),
    }
}

fn fetch_claude_usage() -> Result<String, RealtimeError> {
    let token = dirs::home_dir()
        .map(|home| home.join(".claude").join(".credentials.json"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|credentials| claude_access_token(&credentials))
        .ok_or(RealtimeError::ClaudeCredentialsMissing)?;

    let body = http_get(
        CLAUDE_USAGE_URL,
        &[
            ("Authorization", &format!("Bearer {token}")),
            ("anthropic-beta", CLAUDE_OAUTH_BETA),
        ],
    )
    .map_err(|e| {
        if e.code() == Some(401) {
            RealtimeError::ClaudeSessionExpired
        } else {
            e
        }
    })?;
    claude_usage_sentence(&body, &Local)
}

fn fetch_openai_usage() -> Result<String, RealtimeError> {
    let (path, mut auth) =
        load_codex_auth_for_usage().map_err(|_| RealtimeError::CodexCredentialsMissing)?;
    if jwt_is_expired(&auth.tokens.access_token, now_epoch(), 60.0) {
        refresh_codex_tokens_for_usage(&path, &mut auth)
            .map_err(|_| RealtimeError::CodexSessionExpired)?;
    }

    let query = |auth: &launchtype_core::ai_auth::CodexAuth| {
        let bearer = format!("Bearer {}", auth.tokens.access_token);
        let mut headers: Vec<(&str, &str)> = vec![("Authorization", &bearer)];
        let account_id = auth.tokens.extra.get("account_id").and_then(|v| v.as_str());
        if let Some(account_id) = account_id {
            headers.push(("chatgpt-account-id", account_id));
        }
        http_get(CODEX_USAGE_URL, &headers)
    };

    let body = match query(&auth) {
        Ok(body) => body,
        // The token was rejected despite not looking expired; refresh once.
        Err(e) if e.code() == Some(401) => {
            refresh_codex_tokens_for_usage(&path, &mut auth)
                .map_err(|_| RealtimeError::CodexSessionExpired)?;
            query(&auth)?
        }
        Err(e) => return Err(e),
    };
    openai_usage_sentence(&body, &Local)
}

fn fetch_temperatures() -> Result<String, RealtimeError> {
    let nvidia = run_command("nvidia-smi", &NVIDIA_SMI_ARGS)
        .as_deref()
        .and_then(parse_nvidia_smi);
    let blob = read_windows_sensors();
    let sensors = read_hwmonitor_sensors();
    temperatures_sentence(nvidia.as_ref(), &blob, &sensors)
}

#[cfg(windows)]
fn read_windows_sensors() -> serde_json::Map<String, serde_json::Value> {
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(utf16_le_bytes(SENSORS_POWERSHELL));
    match run_command(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
        ],
    ) {
        Some(output) => parse_windows_sensors(&output),
        None => Default::default(),
    }
}

#[cfg(not(windows))]
fn read_windows_sensors() -> serde_json::Map<String, serde_json::Value> {
    // WMI does not exist off Windows; the sentence builder degrades to
    // "no sensor data" when everything is empty.
    Default::default()
}

#[cfg(windows)]
fn utf16_le_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect()
}

fn read_hwmonitor_sensors() -> Vec<HwSensor> {
    let Ok(body) = http_get_with_timeout(
        HWMONITOR_URL,
        &[],
        Duration::from_secs(HWMONITOR_TIMEOUT_SECONDS),
    ) else {
        return Vec::new();
    };
    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(payload) => collect_hwmonitor_sensors(&payload),
        Err(_) => Vec::new(),
    }
}

/// Run a command hidden and capture stdout; `None` on spawn failure or
/// non-UTF8-decodable output (lossy decoding is applied first).
fn run_command(program: &str, args: &[&str]) -> Option<String> {
    let mut command = std::process::Command::new(program);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().ok()?;
    if !output.status.success() && output.stdout.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}
