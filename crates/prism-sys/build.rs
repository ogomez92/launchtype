use std::env;
use std::path::{Path, PathBuf};

/// `vendor/prism-sdk`, relative to this crate. Holds the Windows and macOS slices
/// the build actually links, so a plain `cargo build` needs no external SDK.
/// Set `PRISM_SDK_DIR` to a full `prism-sdk-vX.Y.Z` for other targets or a newer version.
fn vendored_sdk_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/prism-sdk")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("../../vendor/prism-sdk"))
}

fn main() {
    println!("cargo:rerun-if-env-changed=PRISM_SDK_DIR");
    let sdk = match env::var_os("PRISM_SDK_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => vendored_sdk_dir(),
    };
    if !sdk.is_dir() {
        panic!(
            "Prism SDK not found at {}. Set PRISM_SDK_DIR to the prism-sdk-vX.Y.Z directory.",
            sdk.display()
        );
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    match target_os.as_str() {
        "windows" => {
            let arch = match target_arch.as_str() {
                "x86_64" => "x64",
                "aarch64" => "arm64",
                other => panic!("unsupported Windows arch for prism: {other}"),
            };
            // Dynamic linking: import lib now, prism.dll next to the exe at runtime.
            let libdir = sdk.join(format!(r"windows\{arch}\dynamic\release\lib"));
            let bindir = sdk.join(format!(r"windows\{arch}\dynamic\release\bin"));
            assert_dir(&libdir);
            println!("cargo:rustc-link-search=native={}", libdir.display());
            println!("cargo:rustc-link-lib=dylib=prism");
            copy_runtime_dll(&bindir);
        }
        "macos" => {
            let libdir = sdk.join("macos/universal/static/release/lib");
            assert_dir(&libdir);
            println!("cargo:rustc-link-search=native={}", libdir.display());
            println!("cargo:rustc-link-lib=static=prism");
            // libprism is C++; static archive needs the C++ runtime and Apple speech frameworks.
            println!("cargo:rustc-link-lib=c++");
            println!("cargo:rustc-link-search=framework=/System/Library/Frameworks");
            for fw in ["Foundation", "AVFoundation", "AppKit", "ApplicationServices"] {
                println!("cargo:rustc-link-lib=framework={fw}");
            }
        }
        "linux" => {
            let arch = match target_arch.as_str() {
                "x86_64" => "x64",
                "aarch64" => "arm64",
                other => panic!("unsupported Linux arch for prism: {other}"),
            };
            let libdir = sdk.join(format!("linux/{arch}/dynamic/release/lib"));
            assert_dir(&libdir);
            println!("cargo:rustc-link-search=native={}", libdir.display());
            println!("cargo:rustc-link-lib=dylib=prism");
        }
        other => panic!("unsupported target OS for prism: {other}"),
    }
}

fn assert_dir(p: &Path) {
    if !p.is_dir() {
        panic!("expected Prism SDK directory missing: {}", p.display());
    }
}

/// Copy prism.dll into target/{profile} so `cargo run`/`cargo test` find it.
/// Deploy scripts copy it for real releases; this is a dev convenience only.
fn copy_runtime_dll(bindir: &Path) {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    // OUT_DIR = target/{profile}/build/prism-sys-*/out
    let profile_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("OUT_DIR should sit under target/{profile}/build")
        .to_path_buf();
    let src = bindir.join("prism.dll");
    if src.is_file() {
        let _ = std::fs::copy(&src, profile_dir.join("prism.dll"));
    }
}
