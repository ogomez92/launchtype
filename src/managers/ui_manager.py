import wx
from ui.command_edition_dialog import CommandEditionDialog
from services.runner_service import run_command

class UIManager:
    commands_in_ui = []

    def __init__(self, data):
        self.app = wx.App(False)
        self.frame = wx.Frame(None, -1, "Launchtype")
        self.panel = wx.Panel(self.frame, -1)
        self.data = data

        sizer = wx.BoxSizer(wx.VERTICAL)

        editSizer = wx.BoxSizer(wx.HORIZONTAL)
        editLabel = wx.StaticText(self.panel, label="Input Field")
        self.edit = wx.TextCtrl(self.panel)
        editSizer.Add(editLabel)
        editSizer.Add(self.edit)
        sizer.Add(editSizer)

        self.list = wx.ListBox(self.panel, style=wx.LB_SINGLE)
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

        self.run_button = wx.Button(
            self.panel, wx.ID_OK, "&Run")
        self.app.Bind(wx.EVT_BUTTON, self.run_button_clicked, self.run_button)
        buttonRowSizer.Add(self.run_button)

        sizer.Add(buttonRowSizer)

    def initialize_ui(self):
        self.app.MainLoop()

    def show_alert(title, text):
        dlg = wx.MessageDialog(None, text, title, wx.OK)
        dlg.ShowModal()
        dlg.Destroy()

    def show_error(title, text):
        dlg = wx.MessageDialog(None, text, title, wx.OK | wx.ICON_ERROR)
        dlg.ShowModal()
        dlg.Destroy()

    def add_button_clicked(self, event):
        with CommandEditionDialog(self.frame, self.data) as addDialog:
            addDialog.ShowModal()

        self.update_list()

    def editButtonClicked(self, event):
        pass

    def deleteButtonClicked(self, event):
        pass

    def toggleVisibility(self):
        isVisible = self.frame.IsShown()
        if isVisible:
            self.frame.Hide()

        else:
            self.frame.Show()
            self.edit.SetFocus()
            self.update_list()

    def update_list(self):
        self.commands_in_ui = []
        self.list.Clear()

        for command in self.data.get_commands(self.edit.Value):
            self.commands_in_ui.append(command)
            command_list_string = command['name']
            self.list.Append(command_list_string)

        # Select the first item of the list
        if self.list.GetCount() > 0:
            self.list.Select(0)

    def run_button_clicked(self, event):
        try:
            selected_option_index = self.list.GetSelection()
            selected_option = self.commands_in_ui[selected_option_index]
            selected_command = str(selected_option['path'])
            selected_args = str(selected_option['args'])
            run_command(selected_command, selected_args)
            self.toggleVisibility()
        except Exception as e:
            UIManager.show_error("Oops...", f"Something went wrong while running your command: {e}")

