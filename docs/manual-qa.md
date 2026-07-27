# Manual QA — screen-reader smoke checklist

Run with NVDA on Windows (also spot-check Narrator/JAWS when available) and
VoiceOver on macOS. The Python app at D:\code\tools\launchtype is the
behavioral reference: when in doubt, compare side by side on the same data.

## Startup & window
- [ ] App starts silently in the background with `-m`; without it the window
      shows, plays the "show" sound, and focus lands in the input field
- [ ] "logo" sound plays once on startup
- [ ] Ctrl+Alt+Space toggles the window from anywhere (hidden ⇄ shown)
- [ ] Escape and Alt+F4 hide the window (app keeps running, hotkey still works)
- [ ] Reopening clears the input field and returns to commands mode
      (snippets mode instead when snippets-on-invoke is set)

## Screen reader
- [ ] Typing into the input field echoes normally
- [ ] The results list is announced as "Results" (not "Sort commands by:")
- [ ] Every mode trigger speaks its announcement (- ? . , ' [ ] # + !)
- [ ] Typing a search speaks the first result; multiple results speak
      "{first}, {n} search results shown, use tab and down arrow..."
- [ ] Exact shortcut match plays the "match" sound and shows a single result

## Modes
- [ ] Commands: run (window hides), run_count increments, stats mode reflects it
- [ ] Sort combobox appears only in commands mode; choice persists across restarts
- [ ] Snippets: copy to clipboard with "copy" sound; apple_snippets.plist entries present
- [ ] Clipboard history: items numbered 1-50, re-copying moves to front
- [ ] Steam: games listed, launch works (steam:// URL)
- [ ] Timers: add, toggle (speaks started/stopped, stays open), fires with
      speech + sound, repeating reschedules
- [ ] Alarms: add, toggle (on/off persists), fires at the right minute
- [ ] Notebrook: # then text then Run posts to "feeds"; 401 forgets credentials
- [ ] Realtime: fetch announces "Fetching {name}" then speaks the value
- [ ] Stats: total + most/least used lines, spoken in full (no 40-char clip)
- [ ] Screenshots: capture copies a pasteable FILE to the clipboard
      (paste into Explorer/Finder); describe/regions/grab flows speak results
- [ ] UAC: run_as_admin command shows the elevation prompt
- [ ] SSH: $ with nothing configured shows the "not configured" error and
      drops back to commands mode
- [ ] SSH: $ connects once (speaks "Connecting"/"Connected"); a command
      returns one list row per output line and the cursor lands on the first
- [ ] SSH: a command writing to stderr shows an alert with an OK button
- [ ] SSH: typing does not re-read the transcript; Enter with an empty input
      field copies the selected line
- [ ] SSH: key auth with an OpenSSH key, with an encrypted key (password
      field used as passphrase), and password-only auth
- [ ] SSH: leaving and re-entering $ reuses the connection (no handshake)
- [ ] SSH: `cd /tmp` then `pwd` prints /tmp (state persists across commands)
- [ ] SSH: an alias defined in the remote ~/.zshrc or ~/.bashrc runs; a
      login script that writes to stderr shows one alert right after
      "Connected"
- [ ] SSH: `exit` as a command shows the error alert, and the next command
      reconnects

## Dialogs
- [ ] Add/Edit/Copy command dialogs; OK on default button needs ONE click
- [ ] Delete removes commands, timers, and alarms
- [ ] Copy Args copies and speaks "Arguments copied" / "No arguments"
- [ ] Settings dialog changes apply immediately (sounds toggle)
- [ ] Settings: commands file combobox lists only commands-shaped JSONs;
      switching reloads the list without a restart; a typed new name works
- [ ] Settings: changing the SSH server drops the live connection
- [ ] Settings: SSH password field is masked

## Portable paths (variables)
- [ ] Add dialog: "Path variable..." and "Argument variable..." are distinct
      buttons, each announced by its own label (not two "Variable" buttons)
- [ ] The variable menu is navigable with the arrow keys and announces each
      item as "{{name}} - description (resolved value)"
- [ ] Picking a variable inserts it AT THE CURSOR (not at the end) and leaves
      focus in the field, so typing continues straight after it
- [ ] Each field's menu only ever fills its own field
- [ ] Browse in the Add dialog stores `{{home}}\...` for a file inside the
      user folder; Settings Browse does the same for the SSH key
- [ ] A command with `{{browser}}` and a URL opens in the default browser;
      `{{chrome}}` opens Chrome, and falls back to the default when Chrome is
      not installed
- [ ] OK on a path with a typo'd variable (`{{hom}}`) shows the "no variable
      called" error and stays in the dialog
- [ ] OK accepts `{{browser}}` even though it is not a file on disk
- [ ] Startup: with hardcoded paths present, the dialog lists one row PER RULE
      with its count, all ticked, and the list is reachable with the keyboard
- [ ] Started minimized (`-m` or the setting): the dialog comes to the FRONT
      and the screen reader announces it, rather than opening behind other
      windows unfocused; the main window goes back to hidden afterwards
- [ ] Same when minimized for the shortcut-conflict alert and for the hotkey
      registration error (run a second copy to force one)
- [ ] "Fix selected" rewrites commands.json, speaks "N paths made portable",
      and running a migrated command still launches the right thing
- [ ] Unticking a row leaves those commands alone
- [ ] "Not now" changes nothing and asks again next start
- [ ] "Never ask again" changes nothing and does not ask again; the Settings
      checkbox turns it back on
- [ ] Restarting after a fix does NOT show the dialog again
- [ ] A file whose only problem is unreachable drive letters never opens the
      dialog on its own
- [ ] Elevated (run_as_admin) command with a quoted, spaced argument still
      receives it as one argument

## CLI flags
- [ ] -q silences effect sounds (alerts still audible)
- [ ] -m starts hidden; -s opens in snippets mode
- [ ] -c uses an alternate commands file; -l an alternate Steam library
- [ ] -c wins over the Settings commands file, and Settings does not
      switch it out from under the flag

## Localization
- [ ] Spanish system locale: UI labels, announcements, and AI answers in Spanish
- [ ] Language setting: forcing English on a Spanish system (and the reverse)
      takes effect after a restart, and says so when saved
