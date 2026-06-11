import wx


class SettingsDialog(wx.Dialog):
    def __init__(self, parent, settings_manager):
        super().__init__(parent, title=_("Settings"))
        self.settings_manager = settings_manager

        main_sizer = wx.BoxSizer(wx.VERTICAL)

        self.enable_sounds_checkbox = wx.CheckBox(self, label=_("Enable &sounds"))
        self.enable_sounds_checkbox.SetValue(settings_manager.get("enable_sounds"))
        main_sizer.Add(self.enable_sounds_checkbox, flag=wx.ALL, border=5)

        self.start_minimized_checkbox = wx.CheckBox(self, label=_("Start &minimized"))
        self.start_minimized_checkbox.SetValue(settings_manager.get("start_minimized"))
        main_sizer.Add(self.start_minimized_checkbox, flag=wx.ALL, border=5)

        self.snippets_on_invoke_checkbox = wx.CheckBox(self, label=_("Start in s&nippets mode when invoked"))
        self.snippets_on_invoke_checkbox.SetValue(settings_manager.get("snippets_on_invoke"))
        main_sizer.Add(self.snippets_on_invoke_checkbox, flag=wx.ALL, border=5)

        steam_label = wx.StaticText(self, label=_("Steam &library path:"))
        main_sizer.Add(steam_label, flag=wx.LEFT | wx.RIGHT | wx.TOP, border=5)

        steam_row = wx.BoxSizer(wx.HORIZONTAL)
        self.steam_library_entry = wx.TextCtrl(self, value=settings_manager.get("steam_library"))
        steam_row.Add(self.steam_library_entry, proportion=1, flag=wx.EXPAND | wx.RIGHT, border=5)

        browse_button = wx.Button(self, label=_("&Browse..."))
        browse_button.Bind(wx.EVT_BUTTON, self.browse_for_folder)
        steam_row.Add(browse_button)
        main_sizer.Add(steam_row, flag=wx.EXPAND | wx.ALL, border=5)

        hint = wx.StaticText(self, label=_("Command line flags override these settings for the current run."))
        main_sizer.Add(hint, flag=wx.ALL, border=5)

        button_sizer = wx.BoxSizer(wx.HORIZONTAL)
        button_sizer.AddStretchSpacer()
        ok_button = wx.Button(self, wx.ID_OK, label=_("&OK"))
        ok_button.Bind(wx.EVT_BUTTON, self.ok_clicked)
        ok_button.SetDefault()
        button_sizer.Add(ok_button, flag=wx.RIGHT, border=5)
        cancel_button = wx.Button(self, wx.ID_CANCEL, label=_("&Cancel"))
        button_sizer.Add(cancel_button)
        main_sizer.Add(button_sizer, flag=wx.EXPAND | wx.ALL, border=5)

        self.SetSizerAndFit(main_sizer)

    def browse_for_folder(self, event):
        with wx.DirDialog(self, message=_("Choose Steam library folder"),
                          defaultPath=self.steam_library_entry.GetValue() or "") as dlg:
            if dlg.ShowModal() == wx.ID_OK:
                self.steam_library_entry.SetValue(dlg.GetPath())

    def ok_clicked(self, event):
        self.settings_manager.update({
            "enable_sounds": self.enable_sounds_checkbox.GetValue(),
            "start_minimized": self.start_minimized_checkbox.GetValue(),
            "snippets_on_invoke": self.snippets_on_invoke_checkbox.GetValue(),
            "steam_library": self.steam_library_entry.GetValue(),
        })
        self.settings_manager.save()
        self.EndModal(wx.ID_OK)
