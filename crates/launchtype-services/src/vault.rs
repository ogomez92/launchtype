//! Auto-lock watch thread for the encrypted vault.
//!
//! A timeout that is only noticed the next time the user opens the vault would
//! be a timeout in name only — the key would still be sitting in this process
//! hours later. This thread makes it real: it ticks in the background and wipes
//! the key as soon as the vault has gone unused for long enough, whether or not
//! anyone is looking at the app.
//!
//! It locks silently. Auto-locking happens while the user is off doing
//! something else, and a screen reader announcement that interrupts whatever
//! they *are* doing to report a background event nobody asked about is worse
//! than saying nothing; the next visit to `*` simply asks for the password.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use launchtype_core::clock::Clock;
use launchtype_core::vault::VaultSession;

/// The idle deadline is measured in minutes, so there is nothing to gain from
/// waking up more often than this.
const TICK: Duration = Duration::from_secs(5);

pub struct VaultLocker {
    shutdown_tx: mpsc::Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl VaultLocker {
    pub fn start(session: Arc<Mutex<VaultSession>>, clock: Arc<dyn Clock>) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let handle = std::thread::Builder::new()
            .name("vault-locker".into())
            .spawn(move || loop {
                match shutdown_rx.recv_timeout(TICK) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                let now = clock.now();
                if session.lock().unwrap().expire(now) {
                    log::info!("vault auto-locked after the idle timeout");
                }
            })
            .expect("spawn vault locker thread");
        VaultLocker { shutdown_tx, handle: Some(handle) }
    }

    pub fn stop(&mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for VaultLocker {
    fn drop(&mut self) {
        self.stop();
    }
}
