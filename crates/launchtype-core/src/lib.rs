//! Pure, GUI-free core of Launchtype: data models, fuzzy search, persistence,
//! settings, and the traits (`Speaker`, `Clock`) that services/app implement.
//! Everything here is deterministic and unit-tested; no native dependencies.

pub mod ai_auth;
pub mod alarms;
pub mod clipboard_history;
pub mod imaging;
pub mod clock;
pub mod i18n;
pub mod mode;
pub mod model;
pub mod realtime;
pub mod search;
pub mod settings;
pub mod speech;
pub mod stats;
pub mod steam;
pub mod storage;
pub mod timers;
