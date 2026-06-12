# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Launchtype is a Windows application launcher inspired by macOS's Launchbar. It provides quick access to commands, applications, snippets, and clipboard history via a global hotkey (Ctrl+Alt+Space).

**Key features:**
- Command launcher with fuzzy search (optionally run as administrator)
- Text snippets system (plus import of Apple `apple_snippets.plist`)
- Clipboard history (up to 50 items)
- Steam games launcher (scans installed games)
- Screenshots (window or full screen) copied to the clipboard
- Countdown timers and time-of-day alarms with audible/spoken alerts
- Realtime data mode: live crypto/FX/commodity/IBEX quotes, local weather, news headlines, and Claude subscription usage spoken on demand
- Notebrook quick-note posting (posts to a `feeds` channel; see `notebrook_service` under Services)
- Persistent user settings (`settings.json`)
- Keyboard-driven interface designed for screen reader accessibility
- Audio feedback for UI interactions

## Development Commands

### Dependency Management
```bash
# Install dependencies (uses uv package manager)
uv sync --all-extras

# Run Python commands through uv
uv run python src/main.py
```

### Building and Releasing
```powershell
# One-step build + deploy + relaunch (must run under PowerShell, not Git Bash)
pwsh ./release.ps1
```
`release.ps1` is the normal release path: it syncs the `build` extra, compiles translations (via `compile_translations.py`), runs PyInstaller, copies `sounds/` and `locale/` into `dist\launchtype`, kills any running Launchtype instance, deploys everything to `C:\Users\nitropc\stuff\software\launchtype`, and relaunches the app. After changing code or translations, the change is not live in the installed app until this is re-run.

Manual equivalent if needed:
```bash
uv run pyinstaller ./main.spec
xcopy sounds dist\launchtype\sounds /E /H /C /I
xcopy locale dist\launchtype\locale /E /H /C /I
```

### Testing
```bash
# Smoke-test the fuzzy search algorithm (manual, print-based)
uv run python test_search.py
```
There is no pytest suite. `test_search.py` prints PASS/FAIL lines and expected-vs-got listings for `helpers/search_utility.py`; it always exits 0, so read the output rather than the exit code.

### Code Quality
```bash
# Lint (config in .flake8: max-line-length = 150, ignores E501 and W503)
uv run flake8 src
```
Pre-existing warnings from the vendored `helpers/playsound.py` and the non-Windows `keyboard_handler/osx.py`/`linux.py` are expected noise; lint the files you changed.

### Running the Application
```bash
# Run directly
uv run python src/main.py

# Command line options:
# -m, --start-minimized: Start application minimized
# -s, --snippets-on-invoke: Start in snippets mode instead of commands
# -q, --quiet: Disable all sounds for this run
# -c, --commands [file]: Specify custom commands file (default: commands.json)
# -l, --steam-library [path]: Specify custom Steam library path (default: C:\Program Files (x86)\Steam\steamapps)
```

CLI flags take precedence over `settings.json` for the current run only (see `main.py`, where `effective_*` values combine the two).

## Architecture

### Application Lifecycle
The entry point is `src/main.py` which:
1. Initializes language handler (gettext; installs the global `_()`)
2. Parses command line parameters
3. Loads `SettingsManager` and computes `effective_*` values (CLI overrides settings)
4. Creates/loads commands.json file via DataManager
5. Loads snippets from files
6. Initializes speech service (for screen reader support)
7. Creates UI via UIManager (passed both DataManager and SettingsManager)
8. Registers global hotkey (Ctrl+Alt+Space)
9. Starts wxPython main loop

### Core Components

**DataManager** (`src/managers/data_manager.py`)
- Central data store and dispatcher: `get_data_list_items(search, mode)` routes to the right per-mode getter based on the `UIMode`
- Persists commands to `commands.json` (configurable via -c flag)
- Loads snippets from `snippets/` directory (*.txt files), plus Apple snippets from `apple_snippets.plist` via `helpers/plist_helper.py`
- Owns the clipboard, timer, and alarm services and the Steam scanner
- Fuzzy matching lives in `helpers/search_utility.py` (see Search Algorithm — NOT difflib)

**SettingsManager** (`src/managers/settings_manager.py`)
- Loads/saves user preferences to `settings.json`; only keys present in `DEFAULTS` are persisted (load whitelists against it)
- Holds Notebrook credentials (`notebrook_url`, `notebrook_token`) — `settings.json` is git-ignored, so credentials never enter the repo
- `get/set` accept any key; `update()` is whitelisted to `DEFAULTS`

**UIManager** (`src/managers/ui_manager.py`)
- Manages the single wxPython frame, the input field, the results list, and all dialogs
- A mode is entered by typing its trigger character into the empty input field; `update_list` detects the trigger, switches `self.mode`, and clears the field. See the Mode trigger table below.
- `run_button_clicked` branches on the selected item's `type` (or, for Notebrook, on `self.mode`) to decide what "Run" does. Timers/alarms toggle in place and keep the window open; most other actions hide the window.

**Mode triggers** (`UIMode` enum in `src/enums/ui_mode.py`):

| Char | Mode | Behavior |
|------|------|----------|
| (default) | COMMANDS | Run saved commands/applications |
| `-` | SNIPPETS | Copy a text snippet to the clipboard |
| `?` | CLIPBOARD | Browse/recopy clipboard history |
| `,` | STEAM | Launch an installed Steam game |
| `'` | SCREENSHOTS | Capture window or full screen to clipboard |
| `[` | TIMERS | Add/toggle countdown timers |
| `]` | ALARMS | Add/toggle time-of-day alarms |
| `#` | NOTEBROOK | Post the field text as a note to the `feeds` channel |
| `+` | REALTIME | Fetch a live value (prices, weather, news) and speak it |
| `.` | (returns to) COMMANDS | Leave the current mode |

**Keyboard Handler** (`src/keyboard_handler/`)
- Platform-specific keyboard handling (Windows/Linux/OSX)
- WXKeyboardHandler registers global hotkeys via wx.RegisterHotKey
- Uses custom key constant mappings in `key_constants.py`

**Services** (`src/services/`):
- `runner_service.py`: Executes commands using subprocess.Popen
  - Arguments are comma-separated in the UI
  - Working directory is set to parent of executable path
  - Supports `run_as_admin` (UAC-elevated launch)
- `speech_service.py`: Integrates with screen readers (accessible_output2)
- `clipboard_history.py`: Background thread monitoring clipboard with pyperclip
  - Polls every 0.1s, maintains up to 50 items
  - Persists to `clipboard_history.json`
- `steam_scanner.py`: Scans Steam library for installed games
  - Parses `appmanifest_*.acf` files in steamapps folder
  - Launches games via `steam://rungameid/APPID` URL
- `screenshot_service.py`: Captures the active window or full screen to the clipboard
- `timer_service.py`: Countdown timers (one-shot or repeating). Definitions persist to `timers.json`; live deadlines are in-memory; a background thread fires them.
- `alarm_service.py`: Time-of-day alarms (24h, once per day while enabled), persisted to `alarms.json`; a background thread checks the wall clock once a minute.
- `notebrook_service.py`: Stdlib-`urllib` client for the Notebrook HTTP API (mirrors the Rust `notebroocli`). Single auth header `authorization: <token>`; raises `NotebrookError` carrying a human-readable reason (and `unauthorized` flag on 401).
- `realtime_service.py`: Stdlib-`urllib` fetchers for free keyless APIs — CoinGecko (BTC/ETH in EUR), Frankfurter ECB rates (EUR→USD), Yahoo Finance chart API (Brent `BZ=F`, gold `GC=F`, `^IBEX`), ipinfo.io + Open-Meteo (weather at the user's IP location, Madrid fallback), and RSS headlines (El País, La Vanguardia Catalunya, VilaWeb, BBC World). The `claude` item reads Claude Code's OAuth token from `~/.claude/.credentials.json` and queries `api.anthropic.com/api/oauth/usage` (the same data `/usage` shows) — subscription session/week limits are not accessible via an Anthropic API key. Each fetcher returns a ready-to-speak sentence; failures raise `RealtimeError`. `UIManager.fetch_realtime_value` runs the fetch in a daemon thread, announces "Fetching..." immediately, and speaks the result via `wx.CallAfter`; the window stays open.

Timers and alarms fire through `helpers/alert_notifier.py` (`fire_alert`), which speaks the title/description and plays the timer's custom `.wav` (or a system beep fallback). Timer/alarm background threads are stopped in `UIManager.exit_app`.

### Data Model

**Command structure** (in commands.json):
```python
{
    "path": "C:\\path\\to\\executable.exe",
    "name": "display name",   # lowercase for matching
    "args": "arg1, arg2",      # comma-separated
    "shortcut": "abbr",        # lowercase, exact match takes priority
    "id": "uuid",
    "run_as_admin": false,     # launch elevated via UAC
    "type": "command"          # optional, defaults to "command"
}
```

Timer and alarm item shapes (with their own `type` values) live in `timer_service.py` / `alarm_service.py`; Notebrook notes are not stored locally — the field text is posted directly.

**Snippet structure** (in-memory):
```python
{
    "shortcut": "filename_without_extension",  # lowercase
    "contents": "text content",
    "type": "snippet"
}
```

**Clipboard item structure** (in-memory):
```python
{
    "name": "clipboard text",
    "shortcut": "1",  # 1-indexed position
    "id": "uuid",
    "type": "clip"
}
```

**Steam game structure** (in-memory):
```python
{
    "name": "game name",       # lowercase for matching
    "shortcut": "",            # not used for Steam games
    "id": "uuid",
    "appid": "620",            # Steam app ID
    "type": "steam"
}
```

### Search Algorithm
Implemented in `src/helpers/search_utility.py` (difflib is **not** used). Two layers:
1. **Exact shortcut match** has priority: `check_exact_shortcut_match` returns the single item whose `shortcut` equals the query (triggers the "match" sound).
2. Otherwise `fuzzy_search` does **subsequence** matching — the query characters must appear in order in the target. Results are scored (lower = better) with bonuses for matches at the start of the string and at word boundaries (` -_./\\`), and a penalty for spread; results are sorted by score. Spaces in the query are ignored.

Per-mode getters in `DataManager` call these against the relevant field (command name, snippet shortcut+contents, clipboard text, game name, etc.).

### Audio Feedback
SoundPlayer (`src/helpers/sound_player.py`) provides audio cues:
- "logo": App startup
- "show"/"hide": Window visibility toggle
- "match": Exact shortcut match
- "type": Search results update
- "run": Command execution or Steam game launch
- "copy": Snippet/clipboard item copied

### Accessibility
The application is designed for blind users:
- SpeechService announces UI changes and first search results
- Audio feedback for all interactions
- Keyboard-driven workflow (no mouse required)
- Screen reader compatible via accessible_output2 library

## Localization

**ALWAYS add both English and Spanish strings when building a new feature.** Every user-facing string (UI labels, speech announcements, dialog text, error messages) must be wrapped in `_()` and have a matching Spanish translation.

- English is the source language: the `msgid` (the literal string passed to `_()`) IS the English text, so writing `_("...")` covers English.
- Spanish must be added to `locale/es/LC_MESSAGES/launchtype.po` as a `msgid`/`msgstr` pair, then compiled to `launchtype.mo` (run from the project root; `pybabel` directly may fail under uv on Windows, so invoke the module):
  ```bash
  uv run python -m babel.messages.frontend compile -d locale -D launchtype
  # equivalent helper, used by release.ps1:
  uv run python compile_translations.py
  ```
- Do this as part of the feature, not as a follow-up. A feature is not complete until both languages are present and the `.mo` is recompiled.

## Important Notes

- The application is Windows-focused (uses os.startfile, winsound, win32gui, xcopy in build)
- Language/locale files expected in `locale/` directory
- Audio files expected in `sounds/` directory
- Working directory must contain `snippets/` folder (auto-created if missing)
- PyInstaller spec excludes console window (console=False)
- Dependencies include vendored `accessible_output2` package in project root
- Runtime state is stored as JSON in the working directory: `commands.json`, `clipboard_history.json`, `timers.json`, `alarms.json`, `settings.json`. `.gitignore` excludes `*.json`, so none of these (including Notebrook credentials in `settings.json`) are committed — do not add user data files to the repo.
