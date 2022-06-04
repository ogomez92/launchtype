import wx
from ui.command_edition_dialog import CommandEditionDialog


class UIManager:
    def __init__(self):
        self.app = wx.App(False)
        self.frame = wx.Frame(None, -1, "Launchtype")
        self.panel = wx.Panel(self.frame, -1)

        sizer = wx.BoxSizer(wx.VERTICAL)

        editSizer = wx.BoxSizer(wx.HORIZONTAL)
        editLabel = wx.StaticText(self.panel, label="Input Field")
        self.edit = wx.TextCtrl(self.panel)
        editSizer.Add(editLabel)
        editSizer.Add(self.edit)
        sizer.Add(editSizer)

        self.list = wx.ListView(self.panel)
        sizer.Add(self.list)

        buttonRowSizer = wx.BoxSizer(wx.HORIZONTAL)
        self.add_button = wx.Button(
            self.panel, wx.ID_ADD, "&Add...")
        self.app.Bind(wx.EVT_BUTTON, self.add_button_clicked, self.add_button)
        buttonRowSizer.Add(self.add_button)

        self.edit_button = wx.Button(
            self.panel, wx.ID_EDIT, "&Edit...")
        self.app.Bind(wx.EVT_BUTTON, self.editButtonClicked, self.edit_button)
        buttonRowSizer.Add(self.edit_button)

        self.delete_button = wx.Button(
            self.panel, wx.ID_DELETE, "&Delete")
        self.app.Bind(wx.EVT_BUTTON, self.deleteButtonClicked,
                      self.delete_button)
        buttonRowSizer.Add(self.delete_button)

        sizer.Add(buttonRowSizer)

    def initialize_ui(self):
        self.app.MainLoop()

    def showAlert(self, title, text):
        dlg = wx.MessageDialog(None, text, title, wx.OK)
        dlg.ShowModal()
        dlg.Destroy()

    def add_button_clicked(self, event):
        with CommandEditionDialog(self.frame) as addDialog:
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
