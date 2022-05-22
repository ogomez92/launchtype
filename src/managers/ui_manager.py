import wx

class UIManager:
    def createUI(self):
        app = wx.App()
        frame = wx.Frame(None, title='Launchtype')
        frame.Show()
        app.MainLoop()