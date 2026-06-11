import wx


class AddAlarmDialog(wx.Dialog):
    def __init__(self, parent, data):
        super().__init__(parent, title=_("Add Alarm"))
        self.dataManager = data

        main_sizer = wx.BoxSizer(wx.VERTICAL)

        # Title
        title_sizer = wx.BoxSizer(wx.HORIZONTAL)
        title_sizer.Add(
            wx.StaticText(self, label=_("&Title:")),
            flag=wx.ALL | wx.ALIGN_CENTER_VERTICAL,
            border=5,
        )
        self.title_entry = wx.TextCtrl(self)
        title_sizer.Add(self.title_entry, proportion=1, flag=wx.EXPAND | wx.ALL, border=5)
        main_sizer.Add(title_sizer, flag=wx.EXPAND)

        # Description
        desc_sizer = wx.BoxSizer(wx.HORIZONTAL)
        desc_sizer.Add(
            wx.StaticText(self, label=_("&Description:")),
            flag=wx.ALL | wx.ALIGN_CENTER_VERTICAL,
            border=5,
        )
        self.desc_entry = wx.TextCtrl(self)
        desc_sizer.Add(self.desc_entry, proportion=1, flag=wx.EXPAND | wx.ALL, border=5)
        main_sizer.Add(desc_sizer, flag=wx.EXPAND)

        # Hour (24-hour format)
        hour_sizer = wx.BoxSizer(wx.HORIZONTAL)
        hour_sizer.Add(
            wx.StaticText(self, label=_("&Hour (0-23):")),
            flag=wx.ALL | wx.ALIGN_CENTER_VERTICAL,
            border=5,
        )
        self.hour_spin = wx.SpinCtrl(self, min=0, max=23, initial=8)
        hour_sizer.Add(self.hour_spin, flag=wx.ALL, border=5)
        main_sizer.Add(hour_sizer)

        # Minute
        minute_sizer = wx.BoxSizer(wx.HORIZONTAL)
        minute_sizer.Add(
            wx.StaticText(self, label=_("&Minute (0-59):")),
            flag=wx.ALL | wx.ALIGN_CENTER_VERTICAL,
            border=5,
        )
        self.minute_spin = wx.SpinCtrl(self, min=0, max=59, initial=0)
        minute_sizer.Add(self.minute_spin, flag=wx.ALL, border=5)
        main_sizer.Add(minute_sizer)

        # Sound file
        sound_sizer = wx.BoxSizer(wx.HORIZONTAL)
        sound_sizer.Add(
            wx.StaticText(self, label=_("&Sound file (optional):")),
            flag=wx.ALL | wx.ALIGN_CENTER_VERTICAL,
            border=5,
        )
        self.sound_entry = wx.TextCtrl(self)
        sound_sizer.Add(self.sound_entry, proportion=1, flag=wx.EXPAND | wx.ALL, border=5)
        browse_button = wx.Button(self, wx.ID_OPEN, label=_("&Browse..."))
        browse_button.Bind(wx.EVT_BUTTON, self.browse_for_sound)
        sound_sizer.Add(browse_button, flag=wx.ALL, border=5)
        main_sizer.Add(sound_sizer, flag=wx.EXPAND)

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

    def browse_for_sound(self, event):
        with wx.FileDialog(
            self,
            message=_("Choose a sound file"),
            wildcard=_("Sound files (*.wav)|*.wav|All files (*.*)|*.*"),
            style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST,
        ) as dlg:
            if dlg.ShowModal() == wx.ID_OK:
                self.sound_entry.Value = dlg.GetPath()

    def ok_button_clicked(self, event):
        title = self.title_entry.GetValue()
        if not title:
            wx.MessageBox(
                _("Please enter a title for the alarm."),
                "Error",
                wx.OK | wx.ICON_ERROR,
            )
            return
        self.dataManager.add_alarm(
            title,
            self.desc_entry.GetValue(),
            self.hour_spin.GetValue(),
            self.minute_spin.GetValue(),
            self.sound_entry.GetValue(),
        )
        self.EndModal(wx.ID_OK)

    def cancel_button_clicked(self, event):
        self.EndModal(wx.ID_CANCEL)
