def showMainWindow(frame):
    self.panel = wx.Panel(self.frame, -1)

    self.edit = wx.TextCtrl(self.panel, -1, "", pos=(10, 100))

    self.list = wx.ListView(self.panel, -1, pos=(10, 10), size=(480, 80))

    self.add_button = wx.Button(
        self.panel, -1, "&Add...", pos=(10, 60))
    self.app.Bind(wx.EVT_BUTTON, self.addButtonClicked, self.add_button)

    self.edit_button = wx.Button(
        self.panel, -1, "&Edit...", pos=(10, 80))
    self.app.Bind(wx.EVT_BUTTON, self.editButtonClicked, self.edit_button)

    self.delete_button = wx.Button(
        self.panel, -1, "&Delete", pos=(10, 100))
    self.app.Bind(wx.EVT_BUTTON, self.deleteButtonClicked,
                  self.delete_button)
