import win32gui, win32con


class WindowManager:
    dataManager = None

    def __init__(self, data):
        self.dataManager = data

    def hide_currently_focused_window(self):
        window = win32gui.GetForegroundWindow()
        win32gui.ShowWindow(window, win32con.SW_HIDE)

        # Set the foreground window to first window that is not hidden
