//! Background firing thread for timers and alarms. One thread ticks every
//! second (timer resolution matches Python's 1s loop); alarms are checked
//! every 20th tick (Python's 20s loop).

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use launchtype_core::alarms::AlarmEngine;
use launchtype_core::clock::Clock;
use launchtype_core::timers::TimerEngine;

use crate::alerts::AlertItem;

pub struct Scheduler {
    shutdown_tx: mpsc::Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl Scheduler {
    pub fn start(
        timers: Arc<Mutex<TimerEngine>>,
        alarms: Arc<Mutex<AlarmEngine>>,
        clock: Arc<dyn Clock>,
        on_alert: impl Fn(AlertItem) + Send + 'static,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let handle = std::thread::Builder::new()
            .name("scheduler".into())
            .spawn(move || {
                let mut tick: u32 = 0;
                loop {
                    match shutdown_rx.recv_timeout(Duration::from_secs(1)) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    let now = clock.now();

                    let fired: Vec<AlertItem> = {
                        let mut engine = timers.lock().unwrap();
                        let mut due = engine.due(now);
                        // Stopwatches landing on the same second play one
                        // after the other, and on Windows the last to start
                        // cuts the rest off. Longest period last makes that
                        // the rarer marker: the five-minute sound is heard
                        // over the one-minute tick it lands on.
                        due.sort_by_key(|t| t.period());
                        due.iter()
                            .map(|t| AlertItem::from_timer(t, engine.elapsed_seconds(&t.id, now)))
                            .collect()
                    };
                    for item in fired {
                        on_alert(item);
                    }

                    tick = tick.wrapping_add(1);
                    if tick.is_multiple_of(20) {
                        let fired: Vec<AlertItem> = {
                            let mut engine = alarms.lock().unwrap();
                            engine.due(now).iter().map(AlertItem::from).collect()
                        };
                        for item in fired {
                            on_alert(item);
                        }
                    }
                }
            })
            .expect("spawn scheduler thread");
        Scheduler { shutdown_tx, handle: Some(handle) }
    }

    pub fn stop(&mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::AlertKind;
    use chrono::{Duration as Span, Local, TimeZone};
    use launchtype_core::clock::FakeClock;
    use launchtype_core::timers::{TickUnit, TimerDef, TimerKind};

    /// The elapsed time a stopwatch announces has to make it from the engine
    /// to the alert, which only the scheduler can do.
    #[test]
    fn a_stopwatch_alert_carries_its_elapsed_time() {
        let start = Local.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
        let timer = TimerDef::new(
            "talk".into(),
            String::new(),
            TimerKind::Stopwatch { every: 5, unit: TickUnit::Minute },
            None,
        );
        let id = timer.id.clone();
        let mut engine = TimerEngine::from_defs(vec![timer], start);
        engine.toggle(&id, start);

        let clock = Arc::new(FakeClock::new(start + Span::minutes(5)));
        let (tx, rx) = mpsc::channel();
        let mut scheduler = Scheduler::start(
            Arc::new(Mutex::new(engine)),
            Arc::new(Mutex::new(AlarmEngine::from_defs(Vec::new()))),
            clock,
            move |item| {
                let _ = tx.send(item);
            },
        );
        let item = rx.recv_timeout(Duration::from_secs(5)).expect("the stopwatch fires");
        scheduler.stop();
        assert_eq!(item.kind, AlertKind::Tick { elapsed_seconds: 300 });
    }
}
