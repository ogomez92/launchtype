import wx


class UIManager:
    def __init__(self):
        self.app = wx.App(False)
        self.frame = wx.Frame(None, -1, "Main Window", size=(300, 200))
        self.panel = wx.Panel(self.frame, -1)

        self.button = wx.Button(self.panel, -1, "Add Command", pos=(100, 20))
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
            self.button.SetFocus()
