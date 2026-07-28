# Launchtype

*[Leer en español](README.es.md)*

I wrote this app to quickly launch commands (applications) with or without command line arguments.

I have an app on my mac called [Launchbar](https://www.obdev.at/products/launchbar/index.html) which does this very efficiently, letting me run apps or websites by the use of small commands or abreviations.

I don't like having a cluttered desktop on Windows, and sometimes I have a lot of different websites to run with complicated URLs and I have to find the text file where I have them stored, copy the address to the browser, etc. this is now over.

This is a launcher that can be used with the press of ctrl+alt+space, or ctrl+cmd+space on the Mac (maybe I will make it configurable later).

You can add commands via the UI, for example add chrome.exe using a URL as arguments to run a website, or add your favorite game as the path to directly run that game using a comand.

From the UI you can also copy existing commands, edit them, and delete.

The commands are stored in a commands.json file (or any other file you specify in the command line). which is modifiable via any text editor that supports JSON formatting to make it readable.

> This used to be a Python app. It is now written in Rust (wxWidgets UI through [wxDragon](https://crates.io/crates/wxdragon)), which means a single native executable, no interpreter, no virtualenv and no dependency install on the machine you run it on. It runs on Windows and macOS.

## Installing

Grab the folder produced by a build (see below) and drop it wherever you like — Launchtype is portable. On Windows that folder holds `launchtype.exe`, `prism.dll`, `sounds/` and `locale/`. On macOS it is `Launchtype.app`.

All your data files live **next to the executable** (next to the `.app` bundle on macOS), so the whole thing can sit on a USB stick or in your Dropbox:

`commands.json`, `settings.json`, `timers.json`, `alarms.json`, `clipboard_history.json`, `realtime_history.json`, `snippets/`, `screenshots/`.

Nothing is written to the registry, `AppData` or `~/Library`.

## Building from source

You need:

1. **Rust stable** (1.92 or newer). Install it with [rustup](https://rustup.rs); the pinned toolchain is in `rust-toolchain.toml`.
2. **A C++ toolchain** for wxWidgets: on Windows, the Visual Studio Build Tools with the "Desktop development with C++" workload; on macOS, the Xcode command line tools (`xcode-select --install`).
The Prism speech SDK, used for screen reader output, needs no setup: the Windows and macOS slices the build links are committed under `vendor/prism-sdk/` (~6 MB) and `crates/prism-sys/build.rs` defaults to them. Set `PRISM_SDK_DIR` to a full `prism-sdk-vX.Y.Z` only to build against a different version or a Linux target.

Then:

```bash
cargo build --release -p launchtype
```

The binary lands in `target/release/launchtype` (`.exe` on Windows). During development `cargo run -p launchtype` works too — on Windows the build script copies the Prism DLLs next to the binary so it just runs.

Run the tests with `cargo test`.

### Windows: build, deploy and relaunch

```powershell
pwsh ./scripts/deploy.ps1
```

This builds in release mode, assembles `dist/` (exe + Prism DLLs + `sounds/` + `locale/`), stops the running instance, copies everything to `%USERPROFILE%\stuff\software\launchtype` and relaunches it. Your data files in the target folder are never touched.

### macOS: build the .app bundle

```bash
./scripts/bundle-mac.sh
```

This produces `dist/Launchtype.app`, ad-hoc signed, with `LSUIElement` set so it lives in the background and is summoned by the hotkey rather than showing a Dock icon. The first screenshot will ask for the Screen Recording permission.

The bundle is built for the host architecture only. `libprism.a` is universal, so a universal app is possible by building both `aarch64-apple-darwin` and `x86_64-apple-darwin` and `lipo`-ing the two binaries together; the script does not do this today.

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

## Portable paths (variables)

Launchtype is meant to travel: copy the folder to another machine — or to a Mac — and keep working. Two things normally break that, and both are solved by variables you can put in a command's path or arguments.

A variable is written `{{name}}` and every machine resolves it for itself. The double braces are deliberate: `%name%` would be ambiguous with percent-encoded URLs (`?sex%5B%5D=female`), which are common in real command arguments.

### Folder variables

| Variable | Windows | macOS |
| --- | --- | --- |
| `{{home}}` | `C:\Users\you` | `/Users/you` |
| `{{desktop}}`, `{{documents}}`, `{{downloads}}` | your Desktop / Documents / Downloads | the same folders |
| `{{music}}`, `{{pictures}}`, `{{videos}}` | your Music / Pictures / Videos | the same folders |
| `{{onedrive}}` | your OneDrive folder | your OneDrive folder |
| `{{appdata}}` | `%APPDATA%` (Roaming) | `~/Library/Application Support` |
| `{{localappdata}}` | `%LOCALAPPDATA%` | `~/Library/Application Support` |
| `{{programfiles}}` | `C:\Program Files` | `/Applications` |
| `{{programfiles86}}` | `C:\Program Files (x86)` | `/Applications` |
| `{{programdata}}` | `C:\ProgramData` | `/Library/Application Support` |
| `{{temp}}` | the temp folder | the temp folder |
| `{{launchtype}}` | the folder Launchtype runs from | the same |
| `{{username}}` | your login name on its own, for arguments | the same |

Windows-style backslashes are translated automatically, so `{{home}}\stuff\notes.txt` written on Windows opens `/Users/you/stuff/notes.txt` on a Mac.

### Browser variables

`{{browser}}` is whatever your operating system uses to open a link, so a command with `{{browser}}` as its path and a URL as its argument works everywhere.

`{{chrome}}`, `{{firefox}}`, `{{edge}}`, `{{brave}}`, `{{vivaldi}}`, `{{opera}}` and `{{safari}}` name a specific browser, so you can keep some links in one browser and some in another. Each is looked up in the usual install locations for the current platform, and **falls back to `{{browser}}` when that browser is not installed** — a `{{chrome}}` command still opens on a Mac that only has Safari, rather than failing.

### Adding them

In the Add and Edit Command dialogs, the "Path variable..." and "Argument variable..." buttons open a menu of every variable with its description and what it resolves to on this machine. Choosing one inserts it where the cursor is and leaves you in the field, so you can carry on typing.

The Browse button also does this for you: pick a file inside your user folder and it is stored as `{{home}}\...` rather than with your user name baked in.

### The startup check

When Launchtype finds paths that only work on this machine it offers to replace them, once, at startup. The list is grouped by rule rather than by command — one line reading `C:\Program Files\Google\Chrome\Application\chrome.exe becomes {{chrome}} (189 used)` rather than 189 separate rows — and everything is ticked by default. "Fix selected" rewrites them, "Not now" asks again next time, and "Never ask again" turns the check off (you can turn it back on in Settings).

The same dialog also lists paths that no variable can rescue — a drive letter or a network share that only exists on the machine the command was added on. Those are shown for information only; edit or delete those commands yourself. They never open the dialog on their own, so a few dead drive letters will not nag you at every start.

## Settings

The Settings button in the UI opens a dialog where you can persist the following preferences to `settings.json`:

- Enable sounds
- Start minimized
- Start in snippets mode when invoked
- Check for machine-specific paths at startup (see [Portable paths](#portable-paths-variables))
- Steam library path
- AI model used for screenshot descriptions (Claude Opus, Sonnet or Haiku)
- Interface language (same as the system, English or Spanish) — applied the next time the app starts
- Commands file — a dropdown of every commands-shaped `.json` sitting next to the app, so you can keep separate sets (work, home, a game) and switch between them without restarting. You can also type a new name to start a fresh one.
- SSH server, port, user name, private key file and password, used by SSH mode

Command line flags override these persisted settings for the current run (for example, passing `-q` disables sounds even if the setting is enabled, and passing `-m` starts minimized even if the setting is off).

## Merging another commands file

The "Merge in..." button brings commands over from a second `commands.json` — the one from your other machine, a set a friend sent you, or the work file you keep alongside the home one. Browse to it and, if it really is a commands file, a dialog lists what it holds.

**Only commands you do not already have are listed.** A command counts as one you already have when it shares an id with one of yours, or when it points at the same path with the same arguments under the same display name (letter case and `\` versus `/` do not count as a difference). The dialog says how many were left out for that reason.

Merging is purely additive, and that is the whole design:

- nothing already in your list is edited, re-pointed, renamed or deleted;
- you are never asked whether to replace a command you already have — those are not on the list to begin with;
- the worst a careless merge can do is add rows, which you can then delete.

Everything on the list is ticked to begin with; "Select all" and "Select none" are there for when you only want a few out of a long file. Each row reads as the command's name, its path, and anything the checks turned up about it:

- **its shortcut is already taken** — by one of your commands, or by an earlier command in the same file. Only the first command matching a shortcut ever runs, so the imported copy comes in without one rather than arriving dead; give it a new shortcut from the Edit dialog.
- **an unresolved variable** — a `{{typo}}`, or a variable from a newer version, in the path or the arguments. It would be launched literally.
- **a path that does not exist here** — a program that machine had and this one does not. It is only a warning: import it anyway if you are about to install it.

None of those block an import; they are there so nothing arrives as a surprise. Imported commands start with a run count of zero, since the runs happened on the other machine.

## Snippets

Snippets are pieces of text that, when their filename is typed to the input field of the UI, the content of the file is put in the clipboard.

In order to use snippets, you need to create .txt files in the snippets folder of the app. The "New snipet" button creates one for you, and "Open Snippets folder" opens that folder in your file manager.

The name of the file is its shortcut, without the txt extension, and the content is what gets coppied.

For example, if you have a file called email.txt in the snippets folder which contains the text my_email@gmail.com, whenever you type "email" in the input field of the app and select it from the list by pressing enter, your email will be in the clipboard, my_email@gmail.com.

In order to access snippets you need to be in snippets mode, you can do this by typing a dash character (-) in the input field. This will cause all the commands to be removed form the list and the snippets will show up.

To go back to commands mode, you can press the period key (.). anyway, each time the app is opened by using the launcher hotkey, it is by default in commands mode so nothing needs to be done.

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

Add a timer with the Add button, or change one with the Edit button. The dialog lets you set:

- A **title** and **description** (announced via your screen reader when the timer fires).
- The number of **minutes** to count down.
- A **repeating** checkbox.
- A **sound**, picked from the tones bundled in `sounds/timers/`. Choose "Custom file..." to use any .wav on your system via Browse, or "No sound" for the system beep. Each pick plays as you land on it, so you can arrow through the list to audition the tones.

When a timer fires it speaks its title and description, then keeps playing its sound — or beeping — until you press Ctrl+Alt+Space (Ctrl+Cmd+Space on Mac). One that goes off while you are away from the keyboard is still sounding when you get back, and the hotkey silences it even if a dialog is open.

Editing a timer that is counting down restarts the countdown against the new number of minutes.

Timers display in the list with their current state:

- **Non-repeating timers** show as `stopped` until started. Running them (Enter or Alt+R) starts the countdown; running them again while they are already counting down **resets** the timer. They fire once and then stop.
- **Repeating timers** fire every X minutes until manually disabled. They default to **on**, and running them (Enter or Alt+R) **toggles** them on/off.

To go back to commands mode, press the period key (.).

## Alarms

Alarm mode can be accessed by pressing `]` (right bracket) in the input field. Alarms fire once per day at a specific time of day in 24-hour format.

Add an alarm with the Add button, or change one with the Edit button. The dialog lets you set:

- A **title** and **description** (announced via your screen reader when the alarm fires).
- The **hour** (0-23) and **minute** (0-59).
- A **sound**, picked from the tones bundled in `sounds/alarms/`. Choose "Custom file..." to use any .wav on your system via Browse, or "No sound" for the system beep. Each pick plays as you land on it, so you can arrow through the list to audition the tones.

As with timers, a firing alarm speaks its title and description and then keeps playing its sound — or beeping — until you press Ctrl+Alt+Space (Ctrl+Cmd+Space on Mac).

Editing an alarm keeps it on or off as it was.

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

## SSH mode

Type `$` in the input field to open a remote shell on the server configured in Settings. Launchtype connects once and keeps the connection alive, so only the first command pays for the handshake.

Authentication prefers the private key file when one is set — any format OpenSSH accepts, including `-----BEGIN OPENSSH PRIVATE KEY-----` and older RSA PEM keys. If the key is passphrase-protected, the password field is tried as its passphrase. With no key file, the password is used for password authentication.

Type a command and press Enter. Standard output comes back as one list item per line, which you can arrow through; pressing Enter on a line with an empty input field copies that line to the clipboard. Anything the command wrote to standard error is shown in an alert with an OK button.

All commands run in a single persistent login shell, so `cd`, exported variables and anything your login scripts set up carry over between commands. On connect Launchtype also sources `~/.zshrc` or `~/.bashrc` (with alias expansion enabled), so aliases and PATH additions from your interactive setup work too; anything those scripts write to standard error is shown once as an alert. One exception: a `.bashrc` that begins with an interactivity guard (`case $- in *i*) ;; *) return;; esac`, common on Debian and Ubuntu) still skips itself — put the aliases you want in Launchtype above that guard. There is still no terminal attached — programs that prompt for input or draw full screens (vim, top) will not work.

## Usage stats

Stats mode can be accessed by pressing `!` (exclamation mark) in the input field. It is a read-only list showing how many commands you have run in total, your 10 most used commands and your 10 least used ones.

To go back to commands mode, press the period key (.).

## Emoji mode

Press `:` (colon) in the input field, then type what the emoji *is*. Names are the ones your screen reader uses, so "grinning face" finds 😀 and "red heart" finds ❤️.

You do not have to know the official name. Each emoji also carries its CLDR keywords, so "laugh" reaches "face with tears of joy", "coffee" reaches "hot beverage" and "tada" reaches "party popper". Type a couple of words and the closest match is first; press Enter to copy the emoji to the clipboard, then paste it wherever you were.

Names and keywords follow the app language — in Spanish you search "fuego", "cohete" or "corazón" — and accents are optional, so "corazon" works just as well. The list shows the name only, never the emoji character next to it, because a screen reader would otherwise read the same thing twice on every arrow press.

The whole table is compiled into the app: nothing is downloaded, and Windows and macOS give identical results. There are close to two thousand emoji and the list shows the best 200 matches at a time, so add a word if what you want is not there yet. Skin-tone variants are left out.

## Unit conversion mode

Press `=` (equals) in the input field and type a number. The list becomes that number converted every way there is — `100` gives "100 degrees Celsius = 212 degrees Fahrenheit", then "100 feet = 3048 centimeters", "100 kilograms = 220.4623 pounds" and on down, everyday conversions first. Press Enter on the one you want and the number alone goes to the clipboard, ready to paste. The window stays open, because one conversion is usually followed by another.

The number is not a search: it is what gets converted. Words typed after it are the search. `100 ft cm` goes straight to "100 feet = 3048 centimeters" and `70 kg lb` to "70 kilograms = 154.3236 pounds". The first unit you name is the one you are converting from, so `100 cm ft` is the other direction. Joining words are optional and understood either way — `100 cm to inches` and `100 cm in inches` both work, and so does `5 in cm`, where "in" is inches. Name a single unit, as in `2 kg`, and the list converts it into everything else first, then everything else into it.

Units answer to their symbol as readily as to their name, and every row tells you what that symbol is by putting it in brackets: "100 centimeters (cm) = 39.3701 inches (in)", "10 miles per hour (mph) = 16.0934 kilometers per hour (km/h)". So you learn what to type by arrowing through the list rather than by reading this page. A symbol with a slash works typed either way, `km/h` or `kmh`. The brackets are left off where the name already says it — "1 psi", "42 men's EU shoe size" — since repeating it would only be read out twice.

Longer words match in the middle too, so `gallon` reaches both the US and the imperial one, while a one- or two-letter symbol only matches where a word begins: typing `l` finds liters and pounds (through "lb") without dragging in every calorie and kelvin that happens to contain an l.

A comma is a decimal point (`1,5 kg lb`), the number can be negative (`-40 c f`, which is the one temperature both scales agree on), and the space after it is optional (`100ft cm`).

What is in there: length, mass, temperature, volume (metric, US and imperial, down to teaspoons and cups), area, speed, pressure, energy, power, data — both the gigabyte a disk is sold by and the gibibyte your operating system reports it as — time, angles, torque, and fuel economy, the one that runs backwards, where more miles per gallon is fewer liters per 100 km. A month is the average Gregorian one and a year is 365.2425 days, so "how many hours in a month" has an answer too.

Shoe sizes are in there as well, for men and for women, between EU, UK, US, Japan, China, Korea, Mexico, Brazil, Russia, Australia and India. `42 eu us shoe men` reads across the chart and gives "42 men's EU shoe size = 8.5 men's US shoe size"; sizes that fall between two printed rows are interpolated, so half sizes have an answer. These come from published conversion tables and are as approximate as those tables are — several countries genuinely sell in another country's sizes (Australia and India in UK sizes, China and Korea in Mondopoint millimeters), which is why those rows agree.

Every conversion is a formula compiled into the app, so the mode needs no network and gives the same answer forever. That is also why there are no currencies: an exchange rate is news rather than arithmetic, and `+` mode already fetches the ones worth having.

Names and search words follow the app language, so in Spanish you type `100 pies cm` or `70 kg libras`. The list shows the best 300 rows at a time — with only a number typed those are the everyday conversions first — so add a word if what you want is not among them yet.

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
| `$` | SSH | Run commands on a remote server and read the output |
| `:` | Emoji | Find an emoji by its description and copy it |
| `=` | Unit conversion | Convert a typed number between units, shoe sizes included |
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
