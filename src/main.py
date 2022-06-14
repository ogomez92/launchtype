import time
from managers.ui_manager import UIManager
from managers.data_manager import DataManager
from keyboard_handler.wx_handler import WXKeyboardHandler
from services.speech_service import SpeechService
from helpers.sound_player import SoundPlayer

dataManager = DataManager()
SpeechService().initialize()

if not dataManager.existsCommandsFile():
    dataManager.create_commands_file()

dataManager.loadCommandsFromFile()

uiManager = UIManager(dataManager)
uiManager.toggleVisibility()

try:
    handler = WXKeyboardHandler(uiManager.frame)
    handler.register_key("control+alt+space", uiManager.toggleVisibility)
except Exception as e:
    UIManager.show_error(
        "error", "There was an error registering the hotkey for the program: "+str(e))

SoundPlayer.play("logo")
uiManager.initialize_ui()