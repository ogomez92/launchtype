# Manual QA — screen-reader smoke checklist

Run with NVDA on Windows (also spot-check Narrator/JAWS when available) and
VoiceOver on macOS. The Python app at D:\code\tools\launchtype is the
behavioral reference: when in doubt, compare side by side on the same data.

## Startup & window
- [ ] App starts silently in the background with `-m`; without it the window
      shows, plays the "show" sound, and focus lands in the input field
- [ ] "logo" sound plays once on startup
- [ ] Ctrl+Alt+Space (Ctrl+Cmd+Space on macOS) toggles the window from anywhere (hidden ⇄ shown)
- [ ] Escape and Alt+F4 hide the window (app keeps running, hotkey still works)
- [ ] Reopening clears the input field and returns to commands mode
      (snippets mode instead when snippets-on-invoke is set)

## Screen reader
- [ ] Typing into the input field echoes normally
- [ ] The results list has no label of its own: arrowing through it speaks only
      the items, never a "Results" prefix (and never "Sort commands by:")
- [ ] Every mode trigger speaks its announcement (- ? . , @ ' [ ] # + ! : $ = * _ /)
- [ ] Typing a search speaks the first result; multiple results speak
      "{first}, {n} search results shown, use down arrow..."
- [ ] Down arrow in the input field focuses the result after the selected one
      (first → second); Up arrow the one before; both clamp at the ends
- [ ] Exact shortcut match plays the "match" sound and shows a single result

## Modes
- [ ] Commands: run (window hides), run_count increments, stats mode reflects it
- [ ] Sort combobox appears only in commands mode; choice persists across restarts
- [ ] "Open in Terminal (Alt+T)" appears only in commands mode; on a command
      whose argument is a folder it opens the terminal there, on a file
      argument its containing folder, and with neither it speaks
      "No folder in this command's arguments" (Windows Terminal / Terminal.app)
- [ ] Snippets: copy to clipboard with "copy" sound; apple_snippets.plist entries present
- [ ] Substitution variables (`_`): the user's own `{{name}}` variables, listed
      with Add / Edit / Delete working on them
- [ ] Clipboard history: items numbered 1-50, re-copying moves to front
- [ ] Steam: games listed, launch works (steam:// URL)
- [ ] Applications: `@` announces the mode and lists what the Start Menu lists
      (Windows: desktop programs, Store apps and the synthesised entries like
      Task Manager; macOS: what Launchpad shows)
- [ ] Applications: typing a name filters it, Enter launches the app, plays
      "run" and hides the window — check one desktop program, one Store app
      and one control-panel entry
- [ ] Applications: launching from an elevated Launchtype starts the app
      unelevated (Windows: no second UAC prompt, and the app's Task Manager
      row is not "elevated")
- [ ] Applications: on a Spanish system `administracion` finds "Administración
      de equipos" without the accent, and accented names sort next to their
      neighbours rather than after "z"
- [ ] Applications: install something, then re-enter `@` without restarting —
      the new program is in the list
- [ ] Applications: no Steam game shows up in `@` at all — they belong to `,`
      (Windows puts a Start Menu shortcut next to every installed game). The
      Steam client itself is still there
- [ ] Applications: the Copy Args button reads "Copy program file (Alt+O)" in
      `@` and goes back to "Copy Args" on leaving; Alt+O works under both
- [ ] Applications: Alt+O on a desktop program copies and speaks its .exe
      path, and pasting it into a new command's path field runs the app; on a
      Store app it says "This app has no program file to copy" and the
      clipboard is left alone
- [ ] Timers: add, toggle (speaks started/stopped, stays open), fires with
      speech + sound, repeating reschedules
- [ ] Timers: Edit reopens the timer with its saved values; changing the
      minutes of a running timer restarts the countdown at the new length
- [ ] Alarms: add, toggle (on/off persists), fires at the right minute
- [ ] Alarms: Edit reopens the alarm with its saved values and leaves it on or
      off as it was
- [ ] A fired timer/alarm keeps its sound repeating until Ctrl+Alt+Space
      (Ctrl+Cmd+Space on macOS) — including with "No sound", which repeats the
      beep — and the hotkey silences it even while a dialog is open
- [ ] A second timer coming due while one is sounding takes the sound over
      instead of playing on top of it; typing in the window does not cut the
      repeating alert short
- [ ] Sound dropdown lists the .wav files in sounds/alarms (alarm dialog) and
      sounds/timers (timer dialog)
- [ ] Arrowing through the sound dropdown plays each tone and cuts off the one
      before it; "No sound" is silent; closing the dialog stops the preview
- [ ] A bundled sound still plays after a deploy (it is stored relative to
      sounds/, so the install folder moving must not break it)
- [ ] Notebrook: # then text then Run posts to "feeds"; 401 forgets credentials
- [ ] Realtime: fetch announces "Fetching {name}" then speaks the value
- [ ] Stats: total + most/least used lines, spoken in full (no 40-char clip)
- [ ] Emoji: `:` lists smileys first; "red heart" then Enter copies ❤️ with the
      "copy" sound and hides the window; paste it somewhere to confirm the
      glyph, not the name, landed on the clipboard
- [ ] Emoji: the list reads names only — no emoji glyph, so no double
      announcement ("grinning face", not "grinning face grinning face")
- [ ] Emoji: in Spanish the names are Spanish ("cara sonriendo") and "corazon"
      without the accent finds ❤️
- [ ] Units: `=` then `100` lists conversions of that number, spoken in full
      (no 40-char clip), "100 degrees Celsius = 212 degrees Fahrenheit" first
- [ ] Units: the typed number does NOT filter the list; words after it do
      (`100 ft cm` reaches "100 feet (ft) = 3048 centimeters (cm)" first)
- [ ] Units: each row names the symbol to type in brackets, and typing it
      works (arrow to a row, type its symbol, that row comes back); no
      brackets where the name already says it ("1 psi", not "1 psi (psi)")
- [ ] Units: Enter copies the number alone ("3048", not the sentence) with the
      "copy" sound and leaves the window open for the next conversion
- [ ] Units: Enter with no number typed says "Type a number to convert first"
- [ ] Units: `42 eu us shoe men` gives the men's chart row, and `-40 c f`
      gives -40 back
- [ ] Units: in Spanish the names are Spanish ("100 pies = 3048 centímetros")
      and `1,5 kg lb` reads the comma as a decimal point
- [ ] Screenshots: capture copies a pasteable FILE to the clipboard
      (paste into Explorer/Finder); describe/regions/grab flows speak results
- [ ] Screenshots: typing filters the eight actions like any other mode, and a
      number still jumps straight to that action
- [ ] Screenshots: "grab specific region" opens a one-field dialog on the FIRST
      Enter (not the second), with the field focused and ready to type
- [ ] Screenshots: Cancel in that dialog captures nothing and leaves the
      launcher open on the list; OK hides it and starts the search
- [ ] Screenshots: OK with an empty field says so instead of running
- [ ] Screenshots: with no AI login at all (rename ~/.claude and ~/.codex),
      every AI action brings the window back up with an error dialog naming
      both reasons — it never just goes quiet
- [ ] Screenshots: a failure is spoken as well as shown, and dismissing the
      dialog leaves the window hidden again (it was hidden before the capture)
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

## Path mode (`/`)
Needs ffmpeg on PATH (or named in Settings), and Whisper for the transcription
rows. Work on COPIES: a verified conversion deletes the original by design.

- [ ] Copy a file in Explorer/Finder, press `/`: the mode announces the file's
      name and the list is the actions for it. Copy two files and it announces
      the count instead; copy nothing and it says so and offers only "Read the
      clipboard again"
- [ ] The text forms work too: "Copy as path" for one file and for several at
      once, a path pasted out of a terminal, and a `file://` URL from a browser
- [ ] Clipboard text that is NOT a path (a paragraph, a URL) finds nothing and
      never hangs — especially with a network path in the clipboard
- [ ] Copy something new while the mode is open, then "Read the clipboard
      again": the list is rebuilt and the new file is announced
- [ ] Every row names what it will act on ("Convert song.wav to FLAC",
      "Summarize 3 files with Claude"), and rows that apply to nothing on the
      clipboard are absent — no "convert to MP3" on an MP3, no media rows on a
      text file, "extract the audio track" only for a video
- [ ] Number keys jump straight to a row, as in screenshots mode
- [ ] Convert a WAV to FLAC: the FLAC plays, the WAV is gone, and the list is
      now showing the FLAC
- [ ] Untick "Delete the original file after a verified conversion" in
      Settings: the original survives the next conversion
- [ ] Convert with `song.flac` already sitting there: you get `song (2).flac`
      and the existing file is untouched
- [ ] Convert an MKV to MP3: you get the soundtrack, and the video is unchanged
- [ ] "Extract the audio track" of an MP4 is near-instant, lands as `.m4a`, and
      leaves the video alone
- [ ] Copy a FOLDER: every conversion row is offered and names it ("Convert
      everything in music to MP3"), including for a folder you know is full of
      MP3s — nothing has looked inside it yet. No transcribe, media
      information or Claude rows, and no "extract the audio track"
- [ ] Convert that folder to FLAC: every recording at every depth is converted,
      each output beside its own source in the subfolder it came from, and the
      list afterwards is what was written. Work on a COPY of the folder: the
      originals throughout the tree are deleted, subfolders included
- [ ] Ask a folder of MP3s for MP3: nothing is converted and nothing is
      deleted — no `song (2).mp3` anywhere — and it says so naming the folder
- [ ] Copy a folder AND a loose file together: the row promises both
      ("Convert song.wav and everything in music to MP3") and both are done.
      Two folders read as "everything in 2 folders"
- [ ] A folder with nothing convertible in it, and a folder that has been
      deleted since it was copied, are each reported by name rather than
      passing in silence
- [ ] Put a junk file with a media name in the folder (rename a .txt to .wav):
      it fails on its own line and the rest of the folder still converts
- [ ] A folder on a slow or sleeping network share does not stall the LIST —
      the wait, if any, comes after Enter, with the window still usable
- [ ] Point a conversion at a file that is not really audio (rename a .txt to
      .wav): it reports the failure, no half-written output is left behind, and
      the original is still there
- [ ] Media information reads out duration, codec, sample rate, channels,
      bitrate and size, and the same line is on the clipboard
- [ ] Text information and "Copy the contents" work on a .txt/.md; several
      files are joined under their names
- [ ] With ffmpeg NOT installed (rename it), a conversion says ffmpeg was not
      found and points at Settings — and the mode is usable again afterwards
- [ ] Transcribe a recording: the transcript is on the clipboard, a `.txt` is
      saved beside the recording, the recording is NOT deleted, and the list
      moves on to the transcript
- [ ] With no Whisper installed, the transcribe row explains that Claude cannot
      listen to audio and that Whisper is needed
- [ ] Summarize a .txt and a PDF: the summary is spoken and copied
- [ ] Summarize a recording: it transcribes first, then summarizes
- [ ] "Ask Claude about..." and "Translate..." each ask in a dialog that takes
      ONE Enter to open and one to accept; Cancel does nothing at all
- [ ] Proofread a .txt with mistakes: the corrected text is on the clipboard and
      only a short confirmation is spoken (not the whole document)
- [ ] Open in Visual Studio Code opens every copied file in one window; with
      VS Code not installed it says so
- [ ] Open a terminal: a folder opens itself, a file opens its parent, and five
      files from one folder open ONE terminal
- [ ] While a conversion or transcription is running the window stays usable,
      and a second Enter says "Still working on the last one"
- [ ] Every failure is both spoken AND shown in a dialog (try it with the
      window hidden)

## Encrypted vault (`*`)
- [ ] First `*` on a machine with no `vault/` folder opens "Set up the vault",
      reads the warning that the password cannot be recovered, and refuses a
      password under 8 characters or one that does not match the confirmation
- [ ] Cancelling the setup or the unlock dialog leaves the list showing its one
      row, and Enter on that row opens the same dialog again (never a dead end)
- [ ] After setup: Add stores an entry, the list shows its NAME (never the
      secret), and Enter copies the secret, plays "copy", says "{name} copied"
      and hides the window; paste to confirm the secret landed
- [ ] The secret is NOT in clipboard history: `?` does not list it, and
      `clipboard_history.json` does not contain it — check the file on disk,
      including after copying something else afterwards
- [ ] With the clipboard-clear setting at 30s, the clipboard is empty ~30s after
      the copy; copying something else in the meantime is left alone
- [ ] Shortcuts work like the other modes: an exact shortcut match plays "match"
      and shows the single entry
- [ ] Edit reopens an entry with its name, shortcut and secret; renaming keeps
      the same entry rather than making a second one
- [ ] Delete asks first, and the entry and its `.enc` file are both gone
- [ ] `vault/` holds only `vault.meta` plus one `<uuid>.enc` per entry, and
      neither the entry names nor the secrets are readable in any of them
- [ ] Wrong master password: "error" sound, spoken "That is not the master
      password.", no dialog, and the row is still there to try again
- [ ] "Lock the vault now" locks it; going back into `*` asks for the password
- [ ] "Change the master password" wants the current one, then the new one
      twice; afterwards the old password is refused and every entry still opens
- [ ] Auto-lock: with the timeout at 1 minute, leaving the app alone for a
      minute and coming back to `*` asks for the password again
- [ ] With the timeout at 0, the vault asks for the password on every single
      copy
- [ ] Restarting the app leaves the vault locked (nothing is remembered)
- [ ] Deleting `vault/vault.meta` by hand and pressing `*` warns that N
      encrypted files can no longer be opened before offering a fresh vault
- [ ] Copying the whole `vault/` folder to another machine's Launchtype opens
      with the same password
- [ ] Unlocking takes about half a second (Argon2id) and does not feel hung

## Dialogs
- [ ] Add/Edit/Copy command dialogs; OK on default button needs ONE click
- [ ] Delete removes commands, timers, and alarms
- [ ] Copy Args copies and speaks "Arguments copied" / "No arguments"
- [ ] Settings dialog changes apply immediately (sounds toggle)
- [ ] Settings: commands file combobox lists only commands-shaped JSONs;
      switching reloads the list without a restart; a typed new name works
- [ ] Settings: changing the SSH server drops the live connection
- [ ] Settings: SSH password field is masked
- [ ] Settings: shortening the vault timeout applies to the vault that is
      already unlocked, not just to the next one

## Merging another commands file
- [ ] "Merge in..." sits just before Exit and is reached by Tab like the rest
- [ ] Picking settings.json, a timers.json, or a .txt shows "That file is not
      a Launchtype commands file." and changes nothing
- [ ] Merging the file you are already using shows "Nothing to merge"
- [ ] The list holds ONLY commands you do not have; the header states how many
      were left out for being present already
- [ ] The checkbox list is navigable with the arrow keys, each row announced
      with its name, path and any warning; space toggles a row
- [ ] Everything starts ticked; "Select all" and "Select none" work and are
      announced
- [ ] A row whose shortcut you already use says so and names the owner; after
      importing, that command has NO shortcut and the startup conflict warning
      does not appear
- [ ] Two rows in the same file sharing a shortcut: the first keeps it
- [ ] A row with a `{{typo}}` and a row pointing at a path missing here are
      both flagged, and both still import when ticked
- [ ] "Import selected" adds exactly the ticked rows, speaks "N commands
      merged", and the new commands run
- [ ] Existing commands are byte-identical afterwards: same ids, paths,
      shortcuts and run counts (diff commands.json before/after)
- [ ] Imported commands start at zero uses, and `total_runs` is unchanged
- [ ] Cancel, and unticking everything then importing, both change nothing
- [ ] Merging the same file a second time offers nothing

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
- [ ] Elevated (run_as_admin) command with a quoted, spaced argument still
      receives it as one argument

## Parameters a command or snippet asks for
- [ ] A command with two `{{query}}`s asks twice, announcing "Query parameter 1"
      then "Query parameter 2", each over the ask sound
- [ ] While answering, every keystroke plays the query typing sound instead of
      the match/type sounds, the list stays empty, and a leading `?` or `@` does
      NOT switch mode
- [ ] Escape backs out; the input field goes back to being announced as
      "Input Field" and the next Enter does not launch the abandoned command
- [ ] A snippet holding `{{informe}}` asks "informe, query parameter 1" — the
      NAME first — and puts the filled text on the clipboard with the copy sound
- [ ] The same name twice in one snippet is asked ONCE and filled in both places
- [ ] `{{Informe}}` and `{{ informe }}` are one question, put in the first
      spelling written
- [ ] A snippet holding only `{{fecha}}` never asks; it copies with today's date
      as day/month/year, and `{{date}}` as month/day/year
- [ ] Add Snippet dialog: the help line above Contents is announced, and
      "Insert variable..." lists the clock and your own variables
- [ ] In snippets mode the Add button adds a SNIPPET (not a command), and there
      is no longer a "New snipet" button in any other mode

## Variables of your own (`_`)
- [ ] `_` announces "substitution variables mode" and lists every variable as
      its text with `(name)` after it; searching finds one by either
- [ ] Add opens Add Variable; Edit opens it seeded with the selected one; Delete
      removes it — and the list refreshes each time without leaving the mode
- [ ] Renaming one in Edit does not leave the old name behind
- [ ] Enter opens Edit Variable on the selected one, same as the Edit button
- [ ] A name that is already a Launchtype variable (`home`, `query`, `fecha`) is
      refused with the reason, and the dialog stays open
- [ ] A name with a brace in it is refused
- [ ] The dialog's own "Insert variable..." writes a variable out of the others
- [ ] `snippets/placeholders.json` holds them, is readable by hand, and a
      hand-edited one is picked up on the next use — and does NOT appear in the
      snippets list as a snippet called "placeholders"
- [ ] A variable used in a command's arguments expands at launch
- [ ] A variable holding `{{fecha}}` expands that too, and one holding
      `{{informe}}` makes the snippet using it ask for the informe
- [ ] A variable that names itself comes out as `{{name}}` rather than hanging

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
