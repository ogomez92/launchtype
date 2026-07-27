# Vendored Prism SDK (Windows + macOS)

`crates/prism-sys/build.rs` links against the Prism speech SDK. The full SDK is a
~700 MB tree covering every Apple platform, Android, Linux and wasm, and it is not
published anywhere fetchable, so only the libraries this project actually links are
committed here. `build.rs` defaults `PRISM_SDK_DIR` to this directory, which means
a plain `cargo build` works on a fresh clone with nothing installed and no
environment variable set — including in CI, on both Windows and macOS.

Contents, copied verbatim from prism-sdk-v0.16.7:

    windows/x64/dynamic/release/lib/prism.lib     link-time import lib
    windows/x64/dynamic/release/bin/prism.dll     runtime DLL, shipped next to launchtype.exe
    windows/arm64/dynamic/release/lib/prism.lib   ditto, ARM64
    windows/arm64/dynamic/release/bin/prism.dll
    macos/universal/static/release/lib/libprism.a universal (x86_64+arm64) static archive
    LICENSES/, NOTICE                             upstream attribution

Total ~4 MB. Set `PRISM_SDK_DIR` to a real `prism-sdk-vX.Y.Z` to build against a
different version or a Linux target. The SDK headers are not needed — `prism-sys`
declares its own bindings rather than running bindgen.

## Why these particular files

Windows links Prism dynamically, so it needs both the import lib and the DLL.
macOS links `libprism.a` statically and pulls in the C++ runtime plus the
Foundation, AVFoundation, AppKit and ApplicationServices frameworks instead, so
there is no macOS runtime library to ship.

## Tolk is deliberately not vendored

The SDK ships Tolk (a Windows screen-reader bridge for NVDA and JAWS) and earlier
releases copied `tolk.dll` next to the executable, but nothing in this project
uses it on either platform:

* No Rust source references Tolk, and `build.rs` never linked `tolk.lib` — only
  `prism`.
* `prism.dll`'s PE import table (x64 and ARM64) lists only Windows system DLLs.
  The bytes `tolk` do not appear anywhere in `prism.dll` in either ASCII or
  UTF-16LE, so it is not delay-loaded or `LoadLibrary`'d either.
* Prism carries its own NVDA, JAWS, SAPI and ZoomText support (hence
  `LICENSES/nvdaController`) and talks to UI Automation directly.
* On macOS the built binary has no `libtolk` load command and no dlopen of it;
  Prism reaches VoiceOver through the Apple frameworks.

If a future SDK version does start requiring Tolk, re-add `tolk.lib`/`tolk.dll`
here and restore the copy steps in `scripts/deploy.ps1`, the release workflow and
`copy_runtime_dll` in `crates/prism-sys/build.rs`.

## Licensing

Prism is MPL-2.0; the bundled third-party components carry their own licences, all
reproduced under `LICENSES/`. These binaries already ship in every release, so
committing them changes distribution scope only, not licence obligations.

## Updating

Re-copy the files listed above plus `LICENSES/` and `NOTICE` from the new SDK, and
update the version referenced here.
