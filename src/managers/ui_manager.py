import wx


class UIManager:
    def __init__(self):
        self.app = wx.App(False)
        self.frame = wx.Frame(None, -1, "Launchtype", size=(500, 150))
        self.panel = wx.Panel(self.frame, -1)

        self.edit = wx.TextCtrl(self.panel, -1, "", pos=(10, 100))

        self.list = wx.ListView(self.panel, -1, pos=(10, 10), size=(480, 80))

        self.button = wx.Button(self.panel, -1, "Add Command", pos=(10, 100))
        self.app.Bind(wx.EVT_BUTTON, self.addButtonClicked, self.button)

    def initialize_ui(self):
        self.app.MainLoop()

    def showAlert(self, title, text):
        dlg = wx.MessageDialog(None, text, title, wx.OK)
        dlg.ShowModal()
        dlg.Destroy()

    def addButtonClicked(self):
        print("test")

    def toggleVisibility(self):
        isVisible = self.frame.IsShown()
        print("toggling visibility")
        if isVisible:
            self.frame.Hide()

        else:
            self.frame.Show()
            self.edit.SetFocus()
