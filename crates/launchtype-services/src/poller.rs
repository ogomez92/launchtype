//! Clipboard history poller — the 100ms watch thread of
//! `services/clipboard_history.py`. Pure history logic lives in
//! `launchtype_core::clipboard_history`; this thread feeds it and persists
//! changes to `clipboard_history.json` (a plain JSON array of strings).

use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use clipboard_rs::{Clipboard, ClipboardContext};
use launchtype_core::clipboard_history::ClipboardHistory;
use launchtype_core::storage::atomic_write_json;

pub struct ClipboardPoller {
    shutdown_tx: mpsc::Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl ClipboardPoller {
    pub fn start(history: Arc<Mutex<ClipboardHistory>>, storage_path: PathBuf) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let handle = std::thread::Builder::new()
            .name("clipboard-poller".into())
            .spawn(move || {
                // Any single failed tick (clipboard locked by another process,
                // storage file briefly locked) is skipped; the next tick retries.
                let ctx = ClipboardContext::new().ok();
                loop {
                    match shutdown_rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    let Some(ctx) = ctx.as_ref() else { continue };
                    let Ok(value) = ctx.get_text() else { continue };

                    let snapshot = {
                        let mut history = history.lock().unwrap();
                        if history.observe(&value) {
                            Some(history.items().to_vec())
                        } else {
                            None
                        }
                    };
                    if let Some(items) = snapshot {
                        let _ = atomic_write_json(&storage_path, &items, None);
                    }
                }
            })
            .expect("spawn clipboard poller thread");
        ClipboardPoller { shutdown_tx, handle: Some(handle) }
    }

    pub fn stop(&mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(handle) = self.handle.take() {
            // Python joins with a 2s timeout; the 100ms tick makes a plain
            // join effectively immediate here.
            let _ = handle.join();
        }
    }
}

impl Drop for ClipboardPoller {
    fn drop(&mut self) {
        self.stop();
    }
}
