from managers.command_line_parameters import get_command_line_parameters
from managers.data_manager import DataManager
from managers.settings_manager import SettingsManager
from managers.ui_manager import UIManager

from keyboard_handler.wx_handler import WXKeyboardHandler
from services.speech_service import SpeechService
from helpers.sound_player import SoundPlayer

import language_handler
language_handler.initialize()

command_line = get_command_line_parameters()
settings = SettingsManager()

effective_start_minimized = command_line.start_minimized or settings.get("start_minimized")
effective_enable_sounds = settings.get("enable_sounds") and not command_line.quiet
effective_steam_library = command_line.steam_library or settings.get("steam_library")

SoundPlayer.enabled = effective_enable_sounds

dataManager = DataManager(command_line.commands, effective_steam_library)

SpeechService().initialize()

if not dataManager.existsCommandsFile():
    dataManager.create_commands_file()

dataManager.loadCommandsFromFile()
dataManager.load_snippets_from_files()

uiManager = UIManager(
    dataManager,
    settings,
    cli_snippets_on_invoke=command_line.snippets_on_invoke,
    cli_quiet=command_line.quiet,
)

if not effective_start_minimized:
    uiManager.toggle_visibility()

try:
    handler = WXKeyboardHandler(uiManager.frame)
    handler.register_key("control+alt+space", uiManager.toggle_visibility)
except Exception as e:
    uiManager.show_error(
        "error", language_handler._("There was an error registering the hotkey for the program: ")+str(e))

SoundPlayer.play("logo")

uiManager.initialize_ui()
