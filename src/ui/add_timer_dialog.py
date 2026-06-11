import wx


class AddTimerDialog(wx.Dialog):
    def __init__(self, parent, data):
        super().__init__(parent, title=_("Add Timer"))
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

        # Minutes
        minutes_sizer = wx.BoxSizer(wx.HORIZONTAL)
        minutes_sizer.Add(
            wx.StaticText(self, label=_("&Minutes:")),
            flag=wx.ALL | wx.ALIGN_CENTER_VERTICAL,
            border=5,
        )
        self.minutes_spin = wx.SpinCtrl(self, min=1, max=1440, initial=5)
        minutes_sizer.Add(self.minutes_spin, flag=wx.ALL, border=5)
        main_sizer.Add(minutes_sizer)

        # Repeating
        self.repeating_checkbox = wx.CheckBox(self, label=_("&Repeating (fires every X minutes until disabled)"))
        main_sizer.Add(self.repeating_checkbox, flag=wx.ALL, border=5)

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
                _("Please enter a title for the timer."),
                "Error",
                wx.OK | wx.ICON_ERROR,
            )
            return
        self.dataManager.add_timer(
            title,
            self.desc_entry.GetValue(),
            self.minutes_spin.GetValue(),
            self.repeating_checkbox.GetValue(),
            self.sound_entry.GetValue(),
        )
        self.EndModal(wx.ID_OK)

    def cancel_button_clicked(self, event):
        self.EndModal(wx.ID_CANCEL)
