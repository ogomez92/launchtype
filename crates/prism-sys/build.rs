use std::env;
use std::path::{Path, PathBuf};

const DEFAULT_SDK_DIR: &str = r"D:\code\libs\prism\prism-sdk-v0.16.7";

fn main() {
    println!("cargo:rerun-if-env-changed=PRISM_SDK_DIR");
    let sdk = PathBuf::from(env::var("PRISM_SDK_DIR").unwrap_or_else(|_| DEFAULT_SDK_DIR.into()));
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
            // Dynamic linking: import lib now, prism.dll + tolk.dll next to the exe at runtime.
            let libdir = sdk.join(format!(r"windows\{arch}\dynamic\release\lib"));
            let bindir = sdk.join(format!(r"windows\{arch}\dynamic\release\bin"));
            assert_dir(&libdir);
            println!("cargo:rustc-link-search=native={}", libdir.display());
            println!("cargo:rustc-link-lib=dylib=prism");
            copy_runtime_dlls(&bindir);
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

/// Copy prism.dll/tolk.dll into target/{profile} so `cargo run`/`cargo test` find them.
/// Deploy scripts copy them for real releases; this is a dev convenience only.
fn copy_runtime_dlls(bindir: &Path) {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    // OUT_DIR = target/{profile}/build/prism-sys-*/out
    let profile_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("OUT_DIR should sit under target/{profile}/build")
        .to_path_buf();
    for dll in ["prism.dll", "tolk.dll"] {
        let src = bindir.join(dll);
        if src.is_file() {
            let dst = profile_dir.join(dll);
            let _ = std::fs::copy(&src, &dst);
        }
    }
}
