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
                        engine.due(now).iter().map(AlertItem::from).collect()
                    };
                    for item in fired {
                        on_alert(item);
                    }

                    tick = tick.wrapping_add(1);
                    if tick % 20 == 0 {
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
