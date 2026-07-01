# Launchtype

*[Leer en español](README.es.md)*

I wrote this app to quickly launch commands (applications) with or without command line arguments on Windows.

I have an app on my mac called [Launchbar](https://www.obdev.at/products/launchbar/index.html) which does this very efficiently, letting me run apps or websites by the use of small commands or abreviations.

I don't like having a cluttered desktop on Windows, and sometimes I have a lot of different websites to run with complicated URLs and I have to find the text file where I have them stored, copy the address to the browser, etc. this is now over.

This is a launcher that can be used with the press of ctrl+alt+space (maybe I will make it configurable later).

You can add commands via the UI, for example add chrome.exe using a URL as arguments to run a website, or add your favorite game as the path to directly run that game using a comand.

From the UI you can also copy existing commands, edit them, and delete.

The commands are stored in a commands.json file (or any other file you specify in the command line). which is modifiable via any text editor that supports JSON formatting to make it readable.

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

## Settings

The Settings button in the UI opens a dialog where you can persist the following preferences to `settings.json`:

- Enable sounds
- Start minimized
- Start in snippets mode when invoked
- Steam library path

Command line flags override these persisted settings for the current run (for example, passing `-q` disables sounds even if the setting is enabled, and passing `-m` starts minimized even if the setting is off).

## Snippets

Snippets are pieces of text that, when their filename is typed to the input field of the UI, the content of the file is put in the clipboard.

In order to use snippets, you need to create .txt files in the snippets folder of the app.

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

Screenshot mode can be accessed by pressing ' (apostrophe) in the input field. Two actions are available:

- `w` shortcut: capture the active window.
- `s` shortcut: capture the entire screen.

Selecting one copies the resulting JPEG file to your clipboard so you can paste it into any app that accepts image files.

## Timers

Timer mode can be accessed by pressing `[` (left bracket) in the input field. Timers count down for a number of minutes and then notify you.

Add a timer with the Add button. The Add Timer dialog lets you set:

- A **title** and **description** (announced via NVDA when the timer fires).
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

- A **title** and **description** (announced via NVDA when the alarm fires).
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

## Run as administrator

When adding or editing a command you can tick the "Run as administrator" checkbox. The command will be launched with elevated privileges (a UAC prompt will appear on launch).

## Copying command arguments

Select a command in the list and press `Ctrl+C` (or use the Copy Args button) to copy that command's arguments to the clipboard. Useful for commands that store URLs or long argument strings you want to grab quickly.

## Mode Switching Summary

The app has several modes, each accessed by typing a special character in the input field:

| Character | Mode | Description |
|-----------|------|-------------|
| (default) | Commands | Launch saved commands and applications |
| `-` | Snippets | Copy text snippets to clipboard |
| `?` | Clipboard | Access clipboard history |
| `,` | Steam | Launch installed Steam games |
| `'` | Screenshots | Capture window or full screen to clipboard |
| `[` | Timers | Count down for X minutes (one-shot or repeating) |
| `]` | Alarms | Fire at a time of day (24-hour) |
| `#` | Notebrook | Post a quick note to your Notebrook |
| `+` | Realtime data | Speak live prices, weather, news headlines and computer temperatures |
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

- All UI changes are announced via screen readers (using the accessible_output2 library)
- First search result is automatically spoken
- Fully keyboard-driven interface (no mouse required)
- Audio feedback for all interactions

## Known issues

Alt F4 closes the application and you need to run it again (will fix).
Workaround: Use control alt space to hide its Window, or launch a command. Launching a command makes the window go away.

The visual appearance of the ap might not be up to standards. I'm blind and cannot debug the interface.
Workaround: Open a PR and help me make it better ;)

## TODO

 1. Find a way to prevent alt f4 from closing the window.
 2. Find a way to play audio on windows.
 3. Ensure that pyproject.toml dependencies are properly set up.
 4. Compile this into an executable for windows.
 5. Possibly tweak the search difflib method.
 6. Refactor command and snippet handling in the UI
