# Launchtype

*[Leer en español](README.es.md)*

I wrote this app to quickly launch commands (applications) with or without command line arguments.

I have an app on my mac called [Launchbar](https://www.obdev.at/products/launchbar/index.html) which does this very efficiently, letting me run apps or websites by the use of small commands or abreviations.

I don't like having a cluttered desktop on Windows, and sometimes I have a lot of different websites to run with complicated URLs and I have to find the text file where I have them stored, copy the address to the browser, etc. this is now over.

This is a launcher that can be used with the press of ctrl+alt+space (maybe I will make it configurable later).

You can add commands via the UI, for example add chrome.exe using a URL as arguments to run a website, or add your favorite game as the path to directly run that game using a comand.

From the UI you can also copy existing commands, edit them, and delete.

The commands are stored in a commands.json file (or any other file you specify in the command line). which is modifiable via any text editor that supports JSON formatting to make it readable.

> This used to be a Python app. It is now written in Rust (wxWidgets UI through [wxDragon](https://crates.io/crates/wxdragon)), which means a single native executable, no interpreter, no virtualenv and no dependency install on the machine you run it on. It runs on Windows and macOS.

## Installing

Grab the folder produced by a build (see below) and drop it wherever you like — Launchtype is portable. On Windows that folder holds `launchtype.exe`, `prism.dll`, `tolk.dll`, `sounds/` and `locale/`. On macOS it is `Launchtype.app`.

All your data files live **next to the executable** (next to the `.app` bundle on macOS), so the whole thing can sit on a USB stick or in your Dropbox:

`commands.json`, `settings.json`, `timers.json`, `alarms.json`, `clipboard_history.json`, `realtime_history.json`, `snippets/`, `screenshots/`.

Nothing is written to the registry, `AppData` or `~/Library`.

## Building from source

You need:

1. **Rust stable** (1.92 or newer). Install it with [rustup](https://rustup.rs); the pinned toolchain is in `rust-toolchain.toml`.
2. **A C++ toolchain** for wxWidgets: on Windows, the Visual Studio Build Tools with the "Desktop development with C++" workload; on macOS, the Xcode command line tools (`xcode-select --install`).
3. **The Prism speech SDK** (`prism-sdk-vX.Y.Z`), used for screen reader output. Point `PRISM_SDK_DIR` at it if it is not at the default path baked into `crates/prism-sys/build.rs`.

Then:

```powershell
$env:PRISM_SDK_DIR = "C:\path\to\prism-sdk-v0.16.7"
cargo build --release -p launchtype
```

The binary lands in `target/release/launchtype.exe`. During development `cargo run -p launchtype` works too — the build script copies the Prism DLLs next to the binary so it just runs.

Run the tests with `cargo test`.

### Windows: build, deploy and relaunch

```powershell
pwsh ./scripts/deploy.ps1
```

This builds in release mode, assembles `dist/` (exe + Prism DLLs + `sounds/` + `locale/`), stops the running instance, copies everything to `%USERPROFILE%\stuff\software\launchtype` and relaunches it. Your data files in the target folder are never touched.

### macOS: build the .app bundle

```bash
PRISM_SDK_DIR=/path/to/prism-sdk-v0.16.7 ./scripts/bundle-mac.sh
```

This produces `dist/Launchtype.app`, ad-hoc signed, with `LSUIElement` set so it lives in the background and is summoned by the hotkey rather than showing a Dock icon. The first screenshot will ask for the Screen Recording permission.

### Layout of the code

| Crate | What lives there |
|-------|------------------|
| `crates/launchtype-core` | Data model, storage, search, settings, i18n, realtime data sources — no UI, fully unit tested |
| `crates/launchtype-services` | Side effects: running commands, sounds, clipboard, screenshots, Steam scanning, AI vision, schedulers |
| `crates/launchtype-app` | The wxDragon UI, dialogs, global hotkey, speech |
| `crates/prism`, `crates/prism-sys` | Safe wrapper and bindings for the Prism speech SDK |

Translations are gettext catalogs in `assets/locale/<lang>/LC_MESSAGES/`. `scripts/compile_catalog.py` compiles a `.po` into a `.mo`, and `scripts/check_msgids.py` checks that every translatable string in the code has an entry.

## Usage

The app includes several command line parameters:

- `-m, --start-minimized`: Start the application minimized
- `-s, --snippets-on-invoke`: Start in snippets mode instead of commands mode
- `-q, --quiet`: Disable all sounds for this run
- `-c, --commands [file]`: Specify a custom commands file (default: commands.json)
- `-l, --steam-library [path]`: Specify a custom Steam library path (default: C:\Program Files (x86)\Steam\steamapps)

Once you add a command using the Add button in the UI, in order to use it you can either:

1. Select it from the list
2. Type its shortcut (if any) in the input field of the UI.
3. Type enough letters in the command's display name for it to show up in the list and the screen reader to speak it.

In commands mode a "Sort commands by" combo box lets you order the list by last modified (the default) or by number of uses. The choice is remembered.

## Settings

The Settings button in the UI opens a dialog where you can persist the following preferences to `settings.json`:

- Enable sounds
- Start minimized
- Start in snippets mode when invoked
- Steam library path
- AI model used for screenshot descriptions (Claude Opus, Sonnet or Haiku)

Command line flags override these persisted settings for the current run (for example, passing `-q` disables sounds even if the setting is enabled, and passing `-m` starts minimized even if the setting is off).

## Snippets

Snippets are pieces of text that, when their filename is typed to the input field of the UI, the content of the file is put in the clipboard.

In order to use snippets, you need to create .txt files in the snippets folder of the app. The "New snipet" button creates one for you, and "Open Snippets folder" opens that folder in your file manager.

The name of the file is its shortcut, without the txt extension, and the content is what gets coppied.

For example, if you have a file called email.txt in the snippets folder which contains the text my_email@gmail.com, whenever you type "email" in the input field of the app and select it from the list by pressing enter, your email will be in the clipboard, my_email@gmail.com.

In order to access snippets you need to be in snippets mode, you can do this by typing a dash character (-) in the input field. This will cause all the commands to be removed form the list and the snippets will show up.

To go back to commands mode, you can press the period key (.). anyway, each time the app is opened by using ctrl alt space, it is by default in commands mode so nothing needs to be done.

## Clipboard History

Clipboard history can be accessed by pressing ? (question mark) in the input field. It will show up to 50 text items that you coppied to your clipboard, and it persists across restarts.

It will only work with textual items, not file paths or stuff like that.

## Steam Games Launcher

Steam games launcher mode can be accessed by pressing , (comma) in the input field. This mode scans your Steam library for installed games and lets you launch them directly.

The scanner looks for installed games in your Steam library folder (default: C:\Program Files (x86)\Steam\steamapps) by parsing the appmanifest files. You can specify a custom Steam library path using the `-l` command line option or in the Settings dialog.

Once in Steam mode, you can search for games by name using fuzzy matching, just like with commands. Selecting a game will launch it through Steam.

To go back to commands mode, press the period key (.).

## Screenshots

Screenshot mode can be accessed by pressing ' (apostrophe) in the input field. The window hides itself before capturing, so Launchtype never appears in the shot. Eight actions are available, each with a number as its shortcut:

1. capture the active window to the clipboard.
2. capture the entire screen to the clipboard.
3. describe the active window.
4. describe the entire screen.
5. explore the regions of the active window.
6. explore the regions of the entire screen.
7. grab a specific region of the active window.
8. grab a specific region of the entire screen.

The first two just copy the resulting JPEG file to your clipboard so you can paste it into any app that accepts image files.

**Describe** sends the capture to an AI and speaks back a description written for someone who cannot see the screen.

**Explore regions** asks the AI for up to 8 interesting areas of the capture (dialogs, toolbars, text areas, button groups...) and puts them in a list. Selecting one crops the image to that region and copies the crop to your clipboard.

**Grab specific region** uses whatever you typed in the input field as the thing to find — for example type `the ok button` and pick action 7. If the AI finds it, the crop lands on your clipboard; if not, it says why.

The AI features use **your existing Claude or ChatGPT login**, not an API key: the Claude Code OAuth token from `~/.claude/.credentials.json` first, falling back to the Codex CLI token in `~/.codex/auth.json`. If neither is present, the app tells you so. The model used for Claude is chosen in the Settings dialog.

To go back to commands mode, press the period key (.).

## Timers

Timer mode can be accessed by pressing `[` (left bracket) in the input field. Timers count down for a number of minutes and then notify you.

Add a timer with the Add button. The Add Timer dialog lets you set:

- A **title** and **description** (announced via your screen reader when the timer fires).
- The number of **minutes** to count down.
- A **repeating** checkbox.
- A custom **sound file** (any .wav on your system, chosen via Browse). If no sound is set, the built-in cue is used.

Timers display in the list with their current state:

- **Non-repeating timers** show as `stopped` until started. Running them (Enter or Alt+R) starts the countdown; running them again while they are already counting down **resets** the timer. They fire once and then stop.
- **Repeating timers** fire every X minutes until manually disabled. They default to **on**, and running them (Enter or Alt+R) **toggles** them on/off.

To go back to commands mode, press the period key (.).

## Alarms

Alarm mode can be accessed by pressing `]` (right bracket) in the input field. Alarms fire once per day at a specific time of day in 24-hour format.

Add an alarm with the Add button. The Add Alarm dialog lets you set:

- A **title** and **description** (announced via your screen reader when the alarm fires).
- The **hour** (0-23) and **minute** (0-59).
- A custom **sound file** (any .wav on your system, chosen via Browse). If no sound is set, the built-in cue is used.

Alarms display in the list showing the time and whether they are `on` or `off`. Run an alarm (Enter or Alt+R) to toggle its activation state.

To go back to commands mode, press the period key (.).

## Notebrook notes

Notebrook mode can be accessed by pressing `#` (hash) in the input field. It lets you fire off a quick note to your [Notebrook](https://notebrook.com) account without leaving the launcher.

Type your note and press Enter (or Alt+R). The note is posted to a channel called **feeds**, which is created automatically the first time if it doesn't exist. Whitespace is trimmed, and nothing is sent if the field is empty.

The first time you send a note you'll be asked for your **server URL** and **token** in a two-field dialog. These are stored locally in `settings.json` (which is git-ignored, so they are never committed) and reused afterwards. If the token is ever rejected, the stored credentials are cleared so you'll be asked again on the next attempt.

After sending, the app announces whether the note was sent or, if something went wrong, the reason (network error, bad URL, unauthorized token, etc.).

To go back to commands mode, press the period key (.).

## Realtime data

Realtime data mode can be accessed by pressing `+` (plus) in the input field. It offers live values fetched from free public APIs at the moment you select them:

- `btc`: bitcoin price in euros (CoinGecko)
- `eth`: ethereum price in euros (CoinGecko)
- `usd`: what 1000 euros are worth in US dollars (European Central Bank rates)
- `oil`: brent crude oil price per barrel (Yahoo Finance)
- `gold`: gold price per ounce (Yahoo Finance)
- `ibex`: IBEX 35 stock index (Yahoo Finance)
- `w`: current weather at your location (geolocated by IP, data from Open-Meteo)
- `news`: top headlines from El País
- `cat`: Catalunya headlines from La Vanguardia
- `vila`: headlines in Catalan from VilaWeb
- `bbc`: top world headlines from the BBC
- `cc`: your Claude subscription usage (session and weekly limits, read via Claude Code's local login — no API key needed)
- `t`: your computer's temperatures, fan speeds and GPU (see [Computer temperatures](#computer-temperatures) below)

Press Enter (or Alt+R) on an item: the app announces "Fetching..." and then speaks the live value through your screen reader as soon as it arrives. The window stays open so you can query several values in a row. If a lookup fails (no network, service down), the reason is announced instead.

All the online sources are free and require no API key or account.

To go back to commands mode, press the period key (.).

### Computer temperatures

The `t` item reads your hardware sensors locally (nothing is sent over the network) and speaks a single sentence with your CPU/system temperature, GPU temperature, fan speeds and GPU load — for example: *"Temperatures: CPU 42 degrees. GPU NVIDIA GeForce RTX 5070 at 48 degrees, fan 30 percent, load 5 percent. CPU fan 1200 rpm."*

It gathers whatever your machine exposes, from several sources, and reports only what succeeds:

- **NVIDIA GPU** — read via `nvidia-smi`, which ships with the NVIDIA driver. Gives GPU name, temperature, fan percentage and load. This works out of the box on any machine with an NVIDIA card; no extra software needed.
- **Any GPU** — if no NVIDIA driver is present, the adapter name is read from Windows so you still get "GPU &lt;name&gt;".
- **CPU temperature and fan RPM** — Windows does **not** expose these to normal programs. To read them you need to install and run **LibreHardwareMonitor** with its web server turned on (see below). When it is running, Launchtype automatically picks up its readings; when it is not, the temperature sentence simply omits those parts.

#### Installing LibreHardwareMonitor (optional, for CPU temperature and fan speeds)

LibreHardwareMonitor is a free, open-source hardware monitor. Launchtype does not bundle or require it — install it only if you want CPU temperature and fan RPM in the `t` item.

1. **Install it.** The easiest way is [winget](https://learn.microsoft.com/windows/package-manager/) from a terminal:

   ```powershell
   winget install --id LibreHardwareMonitor.LibreHardwareMonitor -e
   ```

   Or download the ZIP manually from the [LibreHardwareMonitor releases page](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases) and extract it anywhere.

2. **Run it as administrator.** Right-click `LibreHardwareMonitor.exe` → *Run as administrator*. Administrator rights are required for it to load its kernel driver and read CPU temperatures and fan speeds.

3. **Turn on its web server.** In the *Options* menu, open *Remote Web Server* and click *Run* (the default port is 8085). LibreHardwareMonitor then serves every sensor as JSON at `http://localhost:8085/data.json`, which is what Launchtype reads locally — nothing leaves your machine. The setting is remembered, so the server comes back up automatically next time it starts.

4. **Keep it running in the background.** The readings are only available while LibreHardwareMonitor is running. In its *Options* menu you can also enable, so it is always ready after you log in:
   - *Run On Windows Startup*
   - *Start Minimized*
   - *Minimize To Tray* (and *Minimize On Close*)

OpenHardwareMonitor (the older project it was forked from) also works — turn on its *Remote Web Server* (same default port 8085) and Launchtype will read it too.

## Usage stats

Stats mode can be accessed by pressing `!` (exclamation mark) in the input field. It is a read-only list showing how many commands you have run in total, your 10 most used commands and your 10 least used ones.

To go back to commands mode, press the period key (.).

## Run as administrator

When adding or editing a command you can tick the "Run as administrator" checkbox. The command will be launched with elevated privileges (a UAC prompt will appear on launch).

## Copying command arguments

Select a command in the list and press `Alt+O` (or use the Copy Args button) to copy that command's arguments to the clipboard. Useful for commands that store URLs or long argument strings you want to grab quickly.

## Mode Switching Summary

The app has several modes, each accessed by typing a special character in the input field:

| Character | Mode | Description |
|-----------|------|-------------|
| (default) | Commands | Launch saved commands and applications |
| `-` | Snippets | Copy text snippets to clipboard |
| `?` | Clipboard | Access clipboard history |
| `,` | Steam | Launch installed Steam games |
| `'` | Screenshots | Capture, describe or crop a window or the full screen |
| `[` | Timers | Count down for X minutes (one-shot or repeating) |
| `]` | Alarms | Fire at a time of day (24-hour) |
| `#` | Notebrook | Post a quick note to your Notebrook |
| `+` | Realtime data | Speak live prices, weather, news headlines and computer temperatures |
| `!` | Stats | Most and least used commands |
| `.` | (any mode) | Return to Commands mode |

## Audio Feedback

The app provides audio cues for various actions:

- Startup sound when the app launches
- Show/hide sounds when toggling the window
- Match sound when an exact shortcut is found
- Type sound when search results update
- Run sound when executing a command or launching a game
- Copy sound when copying a snippet or clipboard item

Sounds can be turned off via the Settings dialog or by launching the app with `-q`.

## Accessibility

This application is designed with accessibility in mind, particularly for screen reader users:

- All UI changes are announced through your screen reader (via the Prism speech library, which talks to NVDA, JAWS and VoiceOver)
- First search result is automatically spoken
- Fully keyboard-driven interface (no mouse required)
- Audio feedback for all interactions

## Known issues

The visual appearance of the app might not be up to standards. I'm blind and cannot debug the interface.
Workaround: Open a PR and help me make it better ;)

## TODO

 1. Make the global hotkey configurable.
 2. Ship signed, notarized builds for macOS.
 3. More languages beyond English and Spanish.
