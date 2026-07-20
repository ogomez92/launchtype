//! Effectful services: clipboard access + polling, sounds, process launching,
//! snippet/steam loading, timer/alarm scheduling and persistence. Pure logic
//! lives in `launchtype-core`; this crate does the I/O.

pub mod alerts;
pub mod clipboard;
pub mod poller;
pub mod runner;
pub mod scheduler;
pub mod snippets;
pub mod sounds;
pub mod steam;
pub mod stores;
