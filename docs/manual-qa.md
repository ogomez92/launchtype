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

## Dialogs
- [ ] Add/Edit/Copy command dialogs; OK on default button needs ONE click
- [ ] Delete removes commands, timers, and alarms
- [ ] Copy Args copies and speaks "Arguments copied" / "No arguments"
- [ ] Settings dialog changes apply immediately (sounds toggle)

## CLI flags
- [ ] -q silences effect sounds (alerts still audible)
- [ ] -m starts hidden; -s opens in snippets mode
- [ ] -c uses an alternate commands file; -l an alternate Steam library

## Localization
- [ ] Spanish system locale: UI labels, announcements, and AI answers in Spanish
