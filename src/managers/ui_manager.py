import wx
from ui.add_dialog import AddDialog

class UIManager:
    def __init__(self):
        self.app = wx.App(False)
        self.frame = wx.Frame(None, -1, "Launchtype", size=(500, 150))
        self.panel = wx.Panel(self.frame, -1)

        self.edit = wx.TextCtrl(self.panel, -1, "", pos=(10, 100))

        self.list = wx.ListView(self.panel, -1, pos=(10, 10), size=(480, 80))

        self.add_button = wx.Button(
            self.panel, -1, "&Add...", pos=(10, 60))
        self.app.Bind(wx.EVT_BUTTON, self.addButtonClicked, self.add_button)

        self.edit_button = wx.Button(
            self.panel, -1, "&Edit...", pos=(10, 80))
        self.app.Bind(wx.EVT_BUTTON, self.editButtonClicked, self.edit_button)

        self.delete_button = wx.Button(
            self.panel, -1, "&Delete", pos=(10, 100))
        self.app.Bind(wx.EVT_BUTTON, self.deleteButtonClicked,
                      self.delete_button)

    def initialize_ui(self):
        self.app.MainLoop()

    def showAlert(self, title, text):
        dlg = wx.MessageDialog(None, text, title, wx.OK)
        dlg.ShowModal()
        dlg.Destroy()

    def addButtonClicked(self, event):
        with AddDialog(self.frame, "Add Command") as addDialog:
            addDialog.ShowModal()

    def editButtonClicked(self, event):
        print("not implemented")

    def deleteButtonClicked(self, event):
        print("not implemented")

    def toggleVisibility(self):
        isVisible = self.frame.IsShown()
        print("toggling visibility")
        if isVisible:
            self.frame.Hide()

        else:
            self.frame.Show()
            self.edit.SetFocus()
