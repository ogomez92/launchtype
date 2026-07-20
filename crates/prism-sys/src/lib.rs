//! Raw FFI bindings to the Prism screen-reader/TTS SDK.
//! Hand-written against `prism-sdk-v0.16.7/include/prism.h`.
#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int};

pub const PRISM_CONFIG_VERSION: u8 = 2;

/// PrismError (C enum); PRISM_OK == 0.
pub type PrismError = c_int;
pub const PRISM_OK: PrismError = 0;
pub const PRISM_ERROR_NOT_IMPLEMENTED: PrismError = 3;
pub const PRISM_ERROR_ALREADY_INITIALIZED: PrismError = 15;

#[repr(C)]
pub struct PrismConfig {
    pub version: u8,
}

pub enum PrismContext {}
pub enum PrismBackend {}

extern "C" {
    pub fn prism_config_init() -> PrismConfig;
    pub fn prism_init(cfg: *mut PrismConfig) -> *mut PrismContext;
    pub fn prism_shutdown(ctx: *mut PrismContext);

    pub fn prism_registry_create_best(ctx: *mut PrismContext) -> *mut PrismBackend;
    pub fn prism_registry_acquire_best(ctx: *mut PrismContext) -> *mut PrismBackend;

    pub fn prism_backend_free(backend: *mut PrismBackend);
    pub fn prism_backend_name(backend: *mut PrismBackend) -> *const c_char;
    pub fn prism_backend_get_features(backend: *mut PrismBackend) -> u64;
    pub fn prism_backend_initialize(backend: *mut PrismBackend) -> PrismError;
    pub fn prism_backend_speak(
        backend: *mut PrismBackend,
        text: *const c_char,
        interrupt: bool,
    ) -> PrismError;
    pub fn prism_backend_output(
        backend: *mut PrismBackend,
        text: *const c_char,
        interrupt: bool,
    ) -> PrismError;
    pub fn prism_backend_stop(backend: *mut PrismBackend) -> PrismError;

    pub fn prism_error_string(error: PrismError) -> *const c_char;
}
