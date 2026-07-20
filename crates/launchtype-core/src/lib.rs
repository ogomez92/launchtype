//! Pure, GUI-free core of Launchtype: data models, fuzzy search, persistence,
//! settings, and the traits (`Speaker`, `Clock`) that services/app implement.
//! Everything here is deterministic and unit-tested; no native dependencies.

pub mod clock;
pub mod mode;
pub mod model;
pub mod search;
pub mod settings;
pub mod speech;
pub mod storage;
