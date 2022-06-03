from managers.ui_manager import UIManager
from managers.data_manager import DataManager
from keyboard_handler.wx_handler import WXKeyboardHandler

dataManager = DataManager()
uiManager = UIManager()

if not dataManager.existsCommandsFile():
    uiManager.showAlert("Welcome to Launchtype",
                        "I notice that this is the first time using Launchtype. The program hotkey is control + alt + space")
    dataManager.create_commands_file()

uiManager.toggleVisibility()

try:
    handler = WXKeyboardHandler(uiManager.frame)
    handler.register_key("control+alt+space", uiManager.toggleVisibility)
    print("registered")
except Exception as e:
    uiManager.showAlert(
        "error", "There was an error registering the hotkey for the program: "+str(e))

uiManager.initialize_ui()
