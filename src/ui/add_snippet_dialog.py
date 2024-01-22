import wx


class AddSnippetDialog(wx.Dialog):
    global _
    data = None

    def __init__(self, parent, data):
        super().__init__(parent, title=_("Add Snippet"))

        self.dataManager = data

        # Name label and entry
        name_label = wx.StaticText(self, label=_("Name:"))
        self.name_entry = wx.TextCtrl(self)
        name_sizer = wx.BoxSizer(wx.HORIZONTAL)
        name_sizer.Add(name_label, flag=wx.ALIGN_CENTER_VERTICAL | wx.ALL, border=5)
        name_sizer.Add(self.name_entry, proportion=1, flag=wx.EXPAND | wx.ALL, border=5)

        # Contents label and entry
        contents_label = wx.StaticText(self, label=_("Contents:"))
        self.contents_entry = wx.TextCtrl(self, style=wx.TE_MULTILINE)
        contents_sizer = wx.BoxSizer(wx.HORIZONTAL)
        contents_sizer.Add(
            contents_label, flag=wx.ALIGN_CENTER_VERTICAL | wx.ALL, border=5
        )
        contents_sizer.Add(
            self.contents_entry, proportion=1, flag=wx.EXPAND | wx.ALL, border=5
        )

        # OK and Cancel buttons (with the ok and cancel ID)

        ok_button = wx.Button(self, wx.ID_OK, label=_("OK"))
        ok_button.Bind(wx.EVT_BUTTON, self.ok_button_clicked)

        cancel_button = wx.Button(self, wx.ID_CANCEL, label=_("Cancel"))
        cancel_button.Bind(wx.EVT_BUTTON, self.cancel_button_clicked)

        button_sizer = wx.BoxSizer(wx.HORIZONTAL)
        button_sizer.AddStretchSpacer()
        button_sizer.Add(ok_button, flag=wx.ALL, border=5)
        button_sizer.Add(cancel_button, flag=wx.ALL, border=5)

        # Add sizers to dialog
        main_sizer = wx.BoxSizer(wx.VERTICAL)
        main_sizer.Add(name_sizer, flag=wx.EXPAND)
        main_sizer.Add(contents_sizer, proportion=1, flag=wx.EXPAND)
        main_sizer.Add(button_sizer, flag=wx.EXPAND)
        self.SetSizer(main_sizer)
        main_sizer.Fit(self)

        ok_button.SetDefault()

    def ok_button_clicked(self, event):
        name = self.name_entry.GetValue()
        contents = self.contents_entry.GetValue()
        if name and contents:
            self.dataManager.add_snippet(name, contents)
            self.EndModal(wx.ID_OK)
        else:
            wx.MessageBox(
                _("Please enter a name and contents for the snippet."),
                "Error",
                wx.OK | wx.ICON_ERROR,
            )

    def cancel_button_clicked(self, event):
        self.EndModal(wx.ID_CANCEL)
