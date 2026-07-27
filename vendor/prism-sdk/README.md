# Vendored Prism SDK (Windows x64)

`crates/prism-sys/build.rs` links against the Prism speech SDK, which is a local
directory on the developer machine (`PRISM_SDK_DIR`, defaulting to
`D:\code\libs\prism\prism-sdk-v0.16.7`). GitHub-hosted runners have no way to
fetch it, so the Windows x64 dynamic-release slice is committed here and the
release workflow points `PRISM_SDK_DIR` at this directory.

Contents, copied verbatim from prism-sdk-v0.16.7:

    windows/x64/dynamic/release/lib/{prism.lib,tolk.lib}   link-time import libs
    windows/x64/dynamic/release/bin/{prism.dll,tolk.dll}   runtime DLLs, shipped next to launchtype.exe
    LICENSES/, NOTICE                                      upstream attribution

Prism is MPL-2.0; the bundled third-party components carry their own licences,
all reproduced under `LICENSES/`. These DLLs already ship in every release, so
committing them changes distribution scope only, not licence obligations.

Only the slice CI needs is vendored. Local builds still use the full SDK via
`PRISM_SDK_DIR`, which is what `scripts/deploy.ps1` relies on for macOS/Linux
targets and for headers.

## Updating

Re-copy the four binaries plus `LICENSES/` and `NOTICE` from the new SDK, and
update the version referenced above.
