import wx


class CommandEditionDialog(wx.Dialog):
    is_editing = False

    def __init__(self, parent, command_to_edit={}):
        if command_to_edit == {}:
            title = "Add Command"
        else:
            title = "Edit Command"
            is_editing = True
        super(CommandEditionDialog, self).__init__(
            parent, title=title, size=(250, 150))
        sizer = wx.BoxSizer(wx.VERTICAL)
        self.SetSizer(sizer)

        helpLabel = wx.StaticText(
            self, label="Enter the information about the command you wish to add:")
        helpLabel.Wrap(self.GetSize()[0])

        commandEditSizer = wx.BoxSizer(wx.HORIZONTAL)
        commandEditLabel = wx.StaticText(self, label="Command to run:")
        self.command_edit = wx.TextCtrl(self)
        commandEditSizer.Add(commandEditLabel)
        commandEditSizer.Add(self.command_edit)
        sizer.Add(commandEditSizer)

        displayNameEditSizer = wx.BoxSizer(wx.HORIZONTAL)
        displayNameEditLabel = wx.StaticText(self, label="Display Name:")
        self.display_name_edit = wx.TextCtrl(self)
        displayNameEditSizer.Add(displayNameEditLabel)
        displayNameEditSizer.Add(self.display_name_edit)
        sizer.Add(displayNameEditSizer)

        abreviationEditSizer = wx.BoxSizer(wx.HORIZONTAL)
        abreviationEditLabel = wx.StaticText(self, label="Abreviation (optional):")
        self.abreviation_edit = wx.TextCtrl(self)
        abreviationEditSizer.Add(abreviationEditLabel)
        abreviationEditSizer.Add(self.abreviation_edit)
        sizer.Add(abreviationEditSizer)

        buttonRowSizer = wx.BoxSizer(wx.HORIZONTAL)

        self.ok_button = wx.Button(self, wx.ID_OK, label="&OK")
        self.Bind(wx.EVT_BUTTON, self.ok_button_clicked, self.ok_button)
        self.cancel_button = wx.Button(self, wx.ID_CANCEL, label="&Cancel")
        buttonRowSizer.Add(self.ok_button)
        buttonRowSizer.Add(self.cancel_button)

        sizer.Add(buttonRowSizer)

    def ok_button_clicked(self, event):
        print(f"command: {self.command_edit.Value}")
        self.EndModal(wx.ID_OK)

    def cancel_button_clicked(self, event):
        self.EndModal(wx.ID_CANCEL)