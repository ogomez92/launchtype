# Launchtype (Rust)

Rust rewrite of Launchtype — a screen-reader-first keyboard launcher for
Windows and macOS, summoned with **Ctrl+Alt+Space**. Native wxWidgets UI via
[wxDragon], speech through the [Prism] screen-reader SDK (NVDA/JAWS/SAPI on
Windows, VoiceOver/AVSpeech on macOS).

The Python original lives at `D:\code\tools\launchtype` and remains the
behavioral reference until cutover. Data files are byte-compatible: point the
Rust binary at your existing folder and everything carries over.

## Layout

| Crate | Purpose |
|---|---|
| `launchtype-core` | Pure logic: fuzzy search, models, engines, parsers, i18n. No native deps; most tests live here. |
| `launchtype-services` | I/O: HTTP fetchers, AI (Claude/Codex), clipboard polling, sounds, capture, process launch. |
| `launchtype-app` | wxDragon UI shell, dialogs, hotkey, prism speech on the UI thread. |
| `prism-sys` / `prism` | Hand-written FFI + safe wrapper over the Prism SDK. |

## Building (Windows)

Prereqs: Rust (MSVC), Visual Studio 2019+ with Windows SDK, CMake, Ninja,
and the Prism SDK (default `D:\code\libs\prism\prism-sdk-v0.16.7`, override
with `PRISM_SDK_DIR`).

```powershell
cargo test --workspace     # core + services suites
cargo build --release -p launchtype
pwsh ./scripts/deploy.ps1  # build + deploy + relaunch (replaces release.ps1)
```

Dev runs: `cargo run -p launchtype` — prism.dll/tolk.dll are auto-copied to
`target/debug`; copy `assets/sounds` + `assets/locale` there for audio/i18n.

## Building (macOS)

```bash
PRISM_SDK_DIR=/path/to/prism-sdk-v0.16.7 ./scripts/bundle-mac.sh
```

Data files live **next to the .app bundle** (portable by design). The first
screenshot triggers the Screen Recording permission prompt.

## Data files (portable, next to the executable)

`commands.json`, `settings.json`, `timers.json`, `alarms.json`,
`clipboard_history.json`, `realtime_history.json`, `snippets/`,
`screenshots/`, `apple_snippets.plist` — same shapes as the Python app.

## Localization

English msgids live in the source (`tr("...")`); Spanish in
`assets/locale/es/LC_MESSAGES/launchtype.po` (compiled `.mo` shipped).
Every user-facing string must exist in both languages:

```
python scripts/check_msgids.py   # verifies every tr() literal is in the catalog
```

## Manual QA

`docs/manual-qa.md` — the screen-reader smoke checklist (NVDA first).

[wxDragon]: https://github.com/AllenDang/wxDragon
[Prism]: <D:/code/libs/prism>
