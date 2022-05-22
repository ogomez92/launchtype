import wx


class UIManager:
    def createUI(self):
        self.app = wx.App(False)
        self.frame = wx.Frame(None, -1, "Main Window", size=(300, 200))
        self.panel = wx.Panel(self.frame, -1)

        self.button = wx.Button(self.panel, -1, "Add Command", pos=(100, 20))
        self.app.Bind(wx.EVT_BUTTON, self.addButtonClicked, self.button)

    def initialize_ui(self):
        self.app.mainLoop()
        
    def showAlert(self, title, text):
        dlg = wx.MessageDialog(None, title, text, wx.OK)
        dlg.ShowModal()
        dlg.Destroy()

    def addButtonClicked(self):
        print("test")
