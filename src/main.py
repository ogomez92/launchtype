from managers.command_line_parameters import get_command_line_parameters
from managers.data_manager import DataManager
from managers.ui_manager import UIManager

from keyboard_handler.wx_handler import WXKeyboardHandler
from services.speech_service import SpeechService
from helpers.sound_player import SoundPlayer

import language_handler
language_handler.initialize()

command_line = get_command_line_parameters()
dataManager = DataManager(command_line.commands)

SpeechService().initialize()

if not dataManager.existsCommandsFile():
    dataManager.create_commands_file()

dataManager.loadCommandsFromFile()
dataManager.load_snippets_from_files()

uiManager = UIManager(dataManager)

if not command_line.start_minimized:
    uiManager.toggle_visibility()

try:
    handler = WXKeyboardHandler(uiManager.frame)
    handler.register_key("control+alt+space", uiManager.toggle_visibility)
except Exception as e:
    uiManager.show_error(
        "error", language_handler._("There was an error registering the hotkey for the program: ")+str(e))

SoundPlayer.play("logo")

uiManager.initialize_ui()
