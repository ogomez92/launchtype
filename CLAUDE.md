# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Launchtype is a Windows application launcher inspired by macOS's Launchbar. It provides quick access to commands, applications, snippets, and clipboard history via a global hotkey (Ctrl+Alt+Space).

**Key features:**
- Command launcher with fuzzy search
- Text snippets system
- Clipboard history (up to 50 items)
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

### Building
```bash
# Build executable with PyInstaller
uv run pyinstaller ./main.spec

# Copy required assets after building
xcopy sounds dist\launchtype\sounds /E /H /C /I
xcopy locale dist\launchtype\locale /E /H /C /I
```

### Code Quality
```bash
# Linting configuration is in .flake8
# Key settings: max-line-length = 150, ignores E501 and W503
```

### Running the Application
```bash
# Run directly
uv run python src/main.py

# Command line options:
# -m, --start-minimized: Start application minimized
# -s, --snippets-on-invoke: Start in snippets mode instead of commands
# -c, --commands [file]: Specify custom commands file (default: commands.json)
```

## Architecture

### Application Lifecycle
The entry point is `src/main.py` which:
1. Initializes language handler
2. Parses command line parameters
3. Creates/loads commands.json file via DataManager
4. Loads snippets from files
5. Initializes speech service (for screen reader support)
6. Creates UI via UIManager
7. Registers global hotkey (Ctrl+Alt+Space)
8. Starts wxPython main loop

### Core Components

**DataManager** (`src/managers/data_manager.py`)
- Central data store for commands, snippets, and clipboard history
- Persists commands to `commands.json` (configurable via -c flag)
- Loads snippets from `snippets/` directory (*.txt files)
- Uses difflib for fuzzy matching with 0.6 cutoff threshold
- Manages clipboard history service

**UIManager** (`src/managers/ui_manager.py`)
- Manages the wxPython UI frame and controls
- Handles three UI modes (UIMode enum):
  - COMMANDS (default): Shows saved commands
  - SNIPPETS (activated by "-"): Shows text snippets
  - CLIPBOARD (activated by "?"): Shows clipboard history
- Toggle back to COMMANDS mode with "."
- Provides dialogs for adding/editing commands and snippets

**Keyboard Handler** (`src/keyboard_handler/`)
- Platform-specific keyboard handling (Windows/Linux/OSX)
- WXKeyboardHandler registers global hotkeys via wx.RegisterHotKey
- Uses custom key constant mappings in `key_constants.py`

**Services:**
- `runner_service.py`: Executes commands using subprocess.Popen
  - Arguments are comma-separated in the UI
  - Working directory is set to parent of executable path
- `speech_service.py`: Integrates with screen readers
- `clipboard_history.py`: Background thread monitoring clipboard with pyperclip
  - Polls every 0.1s, maintains up to 50 items
  - Persists to `clipboard_history.json`

### Data Model

**Command structure** (in commands.json):
```python
{
    "path": "C:\\path\\to\\executable.exe",
    "name": "display name",  # lowercase for matching
    "args": "arg1, arg2",     # comma-separated
    "shortcut": "abbr",       # lowercase, exact match takes priority
    "id": "uuid",
    "type": "command"         # optional, defaults to "command"
}
```

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

### Search Algorithm
Commands and clipboard items use difflib.get_close_matches with 0.6 cutoff. Shortcuts provide exact match priority (triggers "match" sound vs "type" sound).

### Audio Feedback
SoundPlayer (`src/helpers/sound_player.py`) provides audio cues:
- "logo": App startup
- "show"/"hide": Window visibility toggle
- "match": Exact shortcut match
- "type": Search results update
- "run": Command execution
- "copy": Snippet/clipboard item copied

### Accessibility
The application is designed for blind users:
- SpeechService announces UI changes and first search results
- Audio feedback for all interactions
- Keyboard-driven workflow (no mouse required)
- Screen reader compatible via accessible_output2 library

## Important Notes

- The application is Windows-focused (uses os.startfile, xcopy in build)
- Language/locale files expected in `locale/` directory
- Audio files expected in `sounds/` directory
- Working directory must contain `snippets/` folder (auto-created if missing)
- PyInstaller spec excludes console window (console=False)
- Dependencies include vendored `accessible_output2` package in project root
