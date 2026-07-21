//! Embed a Windows application manifest into the executable.
//!
//! Without this, wxWidgets warns at startup that the app has no manifest
//! declaring Common Controls Library v6. The manifest also opts the process
//! into PerMonitorV2 DPI awareness and the UTF-8 active code page.
#[cfg(windows)]
fn main() {
    use embed_manifest::{embed_manifest, new_manifest};

    // Only the final binary needs the manifest; skip on non-Windows targets.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("LaunchType"))
            .expect("failed to embed Windows application manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}

#[cfg(not(windows))]
fn main() {}
