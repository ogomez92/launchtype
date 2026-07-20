//! Injectable clock so the timer/alarm engines are deterministic under test.

use std::sync::Mutex;

use chrono::{DateTime, Local};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Local>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Local> {
        Local::now()
    }
}

/// Test clock that only moves when told to.
pub struct FakeClock {
    now: Mutex<DateTime<Local>>,
}

impl FakeClock {
    pub fn new(start: DateTime<Local>) -> Self {
        FakeClock { now: Mutex::new(start) }
    }

    pub fn set(&self, t: DateTime<Local>) {
        *self.now.lock().unwrap() = t;
    }

    pub fn advance(&self, d: chrono::Duration) {
        let mut now = self.now.lock().unwrap();
        *now += d;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> DateTime<Local> {
        *self.now.lock().unwrap()
    }
}
