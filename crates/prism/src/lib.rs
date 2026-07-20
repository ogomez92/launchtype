//! Safe wrapper over the Prism screen-reader/TTS SDK.
//!
//! `Speech` is deliberately `!Send`: backend thread-safety is unspecified, so the
//! instance lives on the UI thread and workers post speech requests to it.

use std::ffi::{CStr, CString};
use std::ptr::NonNull;

use prism_sys as sys;

#[derive(Debug, thiserror::Error)]
pub enum PrismInitError {
    #[error("prism_init failed (no context)")]
    Init,
    #[error("no usable speech backend found")]
    NoBackend,
    #[error("backend initialization failed: {0}")]
    BackendInit(String),
}

#[derive(Debug, thiserror::Error)]
#[error("prism call failed: {0}")]
pub struct PrismCallError(String);

pub struct Speech {
    ctx: NonNull<sys::PrismContext>,
    backend: NonNull<sys::PrismBackend>,
    // Raw pointers already make this !Send + !Sync; keep it that way.
}

impl Speech {
    pub fn new() -> Result<Self, PrismInitError> {
        unsafe {
            let mut cfg = sys::prism_config_init();
            cfg.version = sys::PRISM_CONFIG_VERSION;
            let ctx = NonNull::new(sys::prism_init(&mut cfg)).ok_or(PrismInitError::Init)?;

            let backend = NonNull::new(sys::prism_registry_create_best(ctx.as_ptr()))
                .or_else(|| NonNull::new(sys::prism_registry_acquire_best(ctx.as_ptr())));
            let Some(backend) = backend else {
                sys::prism_shutdown(ctx.as_ptr());
                return Err(PrismInitError::NoBackend);
            };

            let err = sys::prism_backend_initialize(backend.as_ptr());
            // create_best can hand back a backend that is already live.
            if err != sys::PRISM_OK && err != sys::PRISM_ERROR_ALREADY_INITIALIZED {
                let msg = error_string(err);
                sys::prism_backend_free(backend.as_ptr());
                sys::prism_shutdown(ctx.as_ptr());
                return Err(PrismInitError::BackendInit(msg));
            }

            Ok(Speech { ctx, backend })
        }
    }

    /// Speak (and braille, where supported) `text`. `interrupt` cuts off current speech.
    pub fn output(&self, text: &str, interrupt: bool) -> Result<(), PrismCallError> {
        let c_text = to_cstring(text);
        unsafe {
            let mut err = sys::prism_backend_output(self.backend.as_ptr(), c_text.as_ptr(), interrupt);
            if err != sys::PRISM_OK {
                // Some backends implement speak but not combined output.
                err = sys::prism_backend_speak(self.backend.as_ptr(), c_text.as_ptr(), interrupt);
            }
            if err != sys::PRISM_OK {
                return Err(PrismCallError(error_string(err)));
            }
        }
        Ok(())
    }

    pub fn stop(&self) {
        unsafe {
            let _ = sys::prism_backend_stop(self.backend.as_ptr());
        }
    }

    pub fn backend_name(&self) -> Option<String> {
        unsafe {
            let p = sys::prism_backend_name(self.backend.as_ptr());
            if p.is_null() {
                None
            } else {
                Some(CStr::from_ptr(p).to_string_lossy().into_owned())
            }
        }
    }
}

impl Drop for Speech {
    fn drop(&mut self) {
        unsafe {
            sys::prism_backend_free(self.backend.as_ptr());
            sys::prism_shutdown(self.ctx.as_ptr());
        }
    }
}

fn to_cstring(text: &str) -> CString {
    CString::new(text).unwrap_or_else(|_| {
        let cleaned: String = text.chars().filter(|&c| c != '\0').collect();
        CString::new(cleaned).expect("NUL-free string")
    })
}

fn error_string(err: sys::PrismError) -> String {
    unsafe {
        let p = sys::prism_error_string(err);
        if p.is_null() {
            format!("error code {err}")
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual smoke test: `cargo test -p prism -- --ignored --nocapture`
    /// Requires a speech backend (screen reader or system TTS) on the machine.
    #[test]
    #[ignore]
    fn speaks_hello() {
        let speech = Speech::new().expect("prism init");
        eprintln!("backend: {:?}", speech.backend_name());
        speech.output("Prism speech test from Rust", true).expect("output");
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}
