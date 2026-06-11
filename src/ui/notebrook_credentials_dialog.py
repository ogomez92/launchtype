import wx


class NotebrookCredentialsDialog(wx.Dialog):
    """Two-field dialog asking for the Notebrook server URL and token.

    Values are read back from ``url`` and ``token`` after a successful
    ShowModal() == wx.ID_OK. They are persisted by the caller (into
    settings.json), never committed to the repository.
    """

    def __init__(self, parent, current_url="", current_token=""):
        super().__init__(parent, title=_("Notebrook credentials"))

        self.url = ""
        self.token = ""

        main_sizer = wx.BoxSizer(wx.VERTICAL)

        main_sizer.Add(
            wx.StaticText(
                self,
                label=_(
                    "Enter your Notebrook server URL and token. "
                    "These are stored locally and only used to send notes."
                ),
            ),
            flag=wx.ALL,
            border=10,
        )

        # Server URL
        url_sizer = wx.BoxSizer(wx.HORIZONTAL)
        url_sizer.Add(
            wx.StaticText(self, label=_("Server &URL:")),
            flag=wx.ALL | wx.ALIGN_CENTER_VERTICAL,
            border=5,
        )
        self.url_entry = wx.TextCtrl(self, value=current_url)
        url_sizer.Add(self.url_entry, proportion=1, flag=wx.EXPAND | wx.ALL, border=5)
        main_sizer.Add(url_sizer, flag=wx.EXPAND)

        # Token
        token_sizer = wx.BoxSizer(wx.HORIZONTAL)
        token_sizer.Add(
            wx.StaticText(self, label=_("&Token:")),
            flag=wx.ALL | wx.ALIGN_CENTER_VERTICAL,
            border=5,
        )
        self.token_entry = wx.TextCtrl(self, value=current_token)
        token_sizer.Add(self.token_entry, proportion=1, flag=wx.EXPAND | wx.ALL, border=5)
        main_sizer.Add(token_sizer, flag=wx.EXPAND)

        # Buttons
        button_sizer = wx.BoxSizer(wx.HORIZONTAL)
        ok_button = wx.Button(self, wx.ID_OK, label=_("&OK"))
        ok_button.Bind(wx.EVT_BUTTON, self.ok_button_clicked)
        ok_button.SetDefault()
        cancel_button = wx.Button(self, wx.ID_CANCEL, label=_("&Cancel"))
        cancel_button.Bind(wx.EVT_BUTTON, self.cancel_button_clicked)
        button_sizer.AddStretchSpacer()
        button_sizer.Add(ok_button, flag=wx.ALL, border=5)
        button_sizer.Add(cancel_button, flag=wx.ALL, border=5)
        main_sizer.Add(button_sizer, flag=wx.EXPAND)

        self.SetSizer(main_sizer)
        main_sizer.Fit(self)

    def ok_button_clicked(self, event):
        url = self.url_entry.GetValue().strip()
        token = self.token_entry.GetValue().strip()

        if not url or not token:
            wx.MessageBox(
                _("Please enter both the server URL and the token."),
                _("Error"),
                wx.OK | wx.ICON_ERROR,
            )
            return

        self.url = url
        self.token = token
        self.EndModal(wx.ID_OK)

    def cancel_button_clicked(self, event):
        self.EndModal(wx.ID_CANCEL)
