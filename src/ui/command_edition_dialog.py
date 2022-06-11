import wx
import os
from managers.data_manager import DataManager

class CommandEditionDialog(wx.Dialog):
    is_editing = False

    def __init__(self, parent, data, command_to_edit={}):
        if command_to_edit == {}:
            title = "Add Command"
        else:
            title = "Edit Command"
            is_editing = True

        self.dataManager = data

        super(CommandEditionDialog, self).__init__(
            parent, title=title, size=(250, 150))
        sizer = wx.BoxSizer(wx.VERTICAL)
        self.SetSizer(sizer)

        helpLabel = wx.StaticText(
            self, label="Enter the information about the command you wish to add:")
        helpLabel.Wrap(self.GetSize()[0])

        commandEditSizer = wx.BoxSizer(wx.HORIZONTAL)
        commandEditLabel = wx.StaticText(self, label="&Path to file:")
        self.command_edit = wx.TextCtrl(self)
        commandEditSizer.Add(commandEditLabel)
        commandEditSizer.Add(self.command_edit)

        browse_button = wx.Button(self, wx.ID_OPEN, label="&Browse...")
        self.Bind(wx.EVT_BUTTON, self.browse_for_file, browse_button)
        commandEditSizer.Add(browse_button)

        sizer.Add(commandEditSizer)

        commandArgsSizer = wx.BoxSizer(wx.HORIZONTAL)
        commandArgsLabel = wx.StaticText(
            self, label="&Arguments (optional, space separated):")
        self.args_edit = wx.TextCtrl(self)
        commandArgsSizer.Add(commandArgsLabel)
        commandArgsSizer.Add(self.args_edit)
        sizer.Add(commandArgsSizer)

        displayNameEditSizer = wx.BoxSizer(wx.HORIZONTAL)
        displayNameEditLabel = wx.StaticText(self, label="Display &Name:")
        self.display_name_edit = wx.TextCtrl(self)
        displayNameEditSizer.Add(displayNameEditLabel)
        displayNameEditSizer.Add(self.display_name_edit)
        sizer.Add(displayNameEditSizer)

        abreviationEditSizer = wx.BoxSizer(wx.HORIZONTAL)
        abreviationEditLabel = wx.StaticText(
            self, label="&Shortcut (optional):")
        self.abreviation_edit = wx.TextCtrl(self)
        abreviationEditSizer.Add(abreviationEditLabel)
        abreviationEditSizer.Add(self.abreviation_edit)
        sizer.Add(abreviationEditSizer)

        buttonRowSizer = wx.BoxSizer(wx.HORIZONTAL)

        self.ok_button = wx.Button(self, wx.ID_OK, label="&OK")
        self.ok_button.SetDefault()
        self.Bind(wx.EVT_BUTTON, self.ok_button_clicked, self.ok_button)
        self.cancel_button = wx.Button(self, wx.ID_CANCEL, label="&Cancel")
        buttonRowSizer.Add(self.ok_button)
        buttonRowSizer.Add(self.cancel_button)

        sizer.Add(buttonRowSizer)

    def ok_button_clicked(self, event):
        if not os.path.exists(self.command_edit.Value):
            with wx.MessageDialog(self, "This path is incorrect.", "Error", wx.OK | wx.ICON_ERROR) as dlg:
                dlg.ShowModal()
            return

        if not self.display_name_edit.Value:
            with wx.MessageDialog(self, "The command must have a display name.", "No display name provided", wx.OK | wx.ICON_ERROR) as dlg:
                dlg.ShowModal()
            return

        print("hello")

        self.dataManager.add_command(self.command_edit.Value, self.display_name_edit.Value, self.args_edit.Value, self.abreviation_edit.Value)
        
        self.EndModal(wx.ID_OK)

    def cancel_button_clicked(self, event):
        self.EndModal(wx.ID_CANCEL)

    def browse_for_file(self, event):
        with wx.FileDialog(
            self, message="Choose a file",
            defaultDir=os.getcwd(),
            defaultFile="",
            wildcard="*.*",
            style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST | wx.FD_SHOW_HIDDEN
        ) as file_dialog:
            if file_dialog.ShowModal() == wx.ID_OK:
                path = file_dialog.GetPath()
                self.command_edit.Value = path
            else:
                print("nope")
