import wx
import os
from managers.data_manager import DataManager


class CommandEditionDialog(wx.Dialog):
    global _
    is_editing = False
    is_copying = False

    def __init__(self, parent, data, command_to_edit={}):
        self.command_to_edit = command_to_edit
        if command_to_edit == {}:
            title = _("Add Command")
        else:
            title = _("Edit Command")
            self.is_editing = True
            if command_to_edit["name"] == "":
                self.is_editing = False
                self.is_copying = True
                title = _("Add command from copy")

        self.dataManager = data

        super(CommandEditionDialog, self).__init__(parent, title=title, size=(250, 150))
        sizer = wx.BoxSizer(wx.VERTICAL)
        self.SetSizer(sizer)

        helpLabel = wx.StaticText(
            self, label=_("Enter the information about the command you wish to add:")
        )
        helpLabel.Wrap(self.GetSize()[0])

        commandEditSizer = wx.BoxSizer(wx.HORIZONTAL)
        commandEditLabel = wx.StaticText(self, label=_("&Path to file:"))
        self.command_edit = wx.TextCtrl(self)
        if not command_to_edit == {} and command_to_edit["path"]:
            self.command_edit.Value = command_to_edit["path"]
        commandEditSizer.Add(commandEditLabel)
        commandEditSizer.Add(self.command_edit)

        browse_button = wx.Button(self, wx.ID_OPEN, label=_("&Browse..."))
        self.Bind(wx.EVT_BUTTON, self.browse_for_file, browse_button)
        commandEditSizer.Add(browse_button)

        sizer.Add(commandEditSizer)

        commandArgsSizer = wx.BoxSizer(wx.HORIZONTAL)
        commandArgsLabel = wx.StaticText(
            self, label=_("&Arguments (optional, comma separated):")
        )
        self.args_edit = wx.TextCtrl(self)
        if not command_to_edit == {} and command_to_edit["args"]:
            self.args_edit.Value = command_to_edit["args"]
        commandArgsSizer.Add(commandArgsLabel)
        commandArgsSizer.Add(self.args_edit)
        sizer.Add(commandArgsSizer)

        displayNameEditSizer = wx.BoxSizer(wx.HORIZONTAL)
        displayNameEditLabel = wx.StaticText(self, label=_("Display &Name:"))
        self.display_name_edit = wx.TextCtrl(self)
        if not command_to_edit == {} and command_to_edit["name"]:
            self.display_name_edit.Value = command_to_edit["name"]
        displayNameEditSizer.Add(displayNameEditLabel)
        displayNameEditSizer.Add(self.display_name_edit)
        sizer.Add(displayNameEditSizer)

        abreviationEditSizer = wx.BoxSizer(wx.HORIZONTAL)
        abreviationEditLabel = wx.StaticText(self, label=_("&Shortcut (optional):"))
        self.abreviation_edit = wx.TextCtrl(self)
        if not command_to_edit == {} and command_to_edit["shortcut"]:
            self.abreviation_edit.Value = command_to_edit["shortcut"]
        abreviationEditSizer.Add(abreviationEditLabel)
        abreviationEditSizer.Add(self.abreviation_edit)
        sizer.Add(abreviationEditSizer)

        buttonRowSizer = wx.BoxSizer(wx.HORIZONTAL)

        self.ok_button = wx.Button(self, wx.ID_OK, label=_("&OK"))
        self.ok_button.SetDefault()
        self.Bind(wx.EVT_BUTTON, self.ok_button_clicked, self.ok_button)
        self.cancel_button = wx.Button(self, wx.ID_CANCEL, label=_("&Cancel"))
        buttonRowSizer.Add(self.ok_button)
        buttonRowSizer.Add(self.cancel_button)

        sizer.Add(buttonRowSizer)

    def ok_button_clicked(self, event):
        if not os.path.exists(self.command_edit.Value):
            with wx.MessageDialog(
                self, _("This path is incorrect."), "Error", wx.OK | wx.ICON_ERROR
            ) as dlg:
                dlg.ShowModal()
            return

        if not self.display_name_edit.Value:
            with wx.MessageDialog(
                self,
                _("The command must have a display name."),
                _("No display name provided"),
                wx.OK | wx.ICON_ERROR,
            ) as dlg:
                dlg.ShowModal()
            return

        if (
            self.dataManager.check_if_shortcut_already_in_commands(
                self.abreviation_edit.Value
            )
            and not self.is_editing
        ):
            with wx.MessageDialog(
                self,
                _("The shortcut is already in use."),
                _("Shortcut taken"),
                wx.OK | wx.ICON_ERROR,
            ) as dlg:
                dlg.ShowModal()
            return

        if not self.command_to_edit == {} and self.is_editing and not self.is_copying:
            self.dataManager.pop_by_uuid(self.command_to_edit["id"])

            if self.command_to_edit["path"] != self.command_edit.Value:
                commands_with_same_path = self.dataManager.get_commands_with_path(
                    self.command_to_edit["path"]
                )

                if len(commands_with_same_path) > 0:
                    actions_to_display = ""
                    for action in commands_with_same_path[:5]:
                        actions_to_display += action["name"] + ", "

                    if len(commands_with_same_path) > 5:
                        actions_to_display += (
                            _("and ")
                            + str(len(commands_with_same_path) - 5)
                            + _(" more. ")
                        )

                    answer = self.show_question_dialog(
                        _("Edit Assistant"),
                        _("This path is already in use by the following actions: ")
                        + actions_to_display
                        + _("Do you want to change the path for all of them?"),
                    )

                    if answer:
                        for action in commands_with_same_path:
                            action["path"] = self.command_edit.Value

                        DataManager().syncCommandsToStorage()

        self.dataManager.add_command(
            self.command_edit.Value,
            self.display_name_edit.Value,
            self.args_edit.Value,
            self.abreviation_edit.Value,
        )

        self.EndModal(wx.ID_OK)

    def cancel_button_clicked(self, event):
        self.EndModal(wx.ID_CANCEL)

    def browse_for_file(self, event):
        with wx.FileDialog(
            self,
            message=_("Choose a file"),
            defaultDir=os.getcwd(),
            defaultFile="",
            wildcard="*.*",
            style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST | wx.FD_SHOW_HIDDEN,
        ) as file_dialog:
            if file_dialog.ShowModal() == wx.ID_OK:
                path = file_dialog.GetPath()
                self.command_edit.Value = path

    def show_question_dialog(self, title, text):
        dlg = wx.MessageDialog(self, text, title, wx.YES_NO | wx.ICON_QUESTION)
        result = dlg.ShowModal()
        dlg.Destroy()
        print(result)
        return result == wx.ID_YES
