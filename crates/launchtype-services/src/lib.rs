//! Effectful services: clipboard access + polling, sounds, process launching,
//! snippet/steam loading, timer/alarm scheduling and persistence. Pure logic
//! lives in `launchtype-core`; this crate does the I/O.

pub mod ai;
pub mod alerts;
pub mod clipboard;
pub mod notebrook;
pub mod poller;
pub mod realtime;
pub mod runner;
pub mod scheduler;
pub mod screenshot;
pub mod snippets;
pub mod sounds;
pub mod steam;
pub mod stores;

/// Shared User-Agent for every outbound HTTP request (same string as the
/// Python realtime service).
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) launchtype";
