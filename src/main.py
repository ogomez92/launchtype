from managers.ui_manager import UIManager
from managers.data_manager import DataManager

dataManager = DataManager()
uiManager = UIManager()

uiManager.createUI()
print("hi")
if not dataManager.existsCommandsFile():
    print("test")
    uiManager.showAlert("fuck you", "title")