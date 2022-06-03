import wx


class AddDialog(wx.Dialog):
    def __init__(self, parent, title):
        super(AddDialog, self).__init__(parent, title=title, size=(250, 150))
        sizer = wx.BoxSizer(wx.VERTICAL)
        self.SetSizer(sizer)

        helpLabel = wx.StaticText(self, label="Enter the information about the command you wish to add:")
        helpLabel.Wrap(self.GetSize()[0])

        commandEditSizer = wx.BoxSizer(wx.HORIZONTAL)
        commandEditLabel = wx.StaticText(self, label="Command to run:")
        self.edit = wx.TextCtrl(self)
        commandEditSizer.Add(commandEditLabel)
        commandEditSizer.Add(self.edit)

        buttonRowSizer = wx.BoxSizer(wx.HORIZONTAL)

        self.okButton = wx.Button(self, wx.ID_OK, label="&OK")

        buttonRowSizer.Add(self.okButton)
        sizer.Add(commandEditSizer)
        sizer.Add(buttonRowSizer)