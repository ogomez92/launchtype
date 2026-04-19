import os
import time
import tempfile
import ctypes
import ctypes.wintypes
import struct
from PIL import ImageGrab


def _copy_file_to_clipboard(file_path):
    """Copy a file handle to the Windows clipboard using CF_HDROP format."""
    CF_HDROP = 15
    GHND = 0x0042

    user32 = ctypes.windll.user32
    kernel32 = ctypes.windll.kernel32

    file_path_w = file_path + "\0"
    file_bytes = file_path_w.encode("utf-16-le")

    # DROPFILES struct: 20 bytes header + file paths (double-null terminated)
    # offset (4) + pt.x (4) + pt.y (4) + fNC (4) + fWide (4) = 20 bytes
    header = struct.pack("IIIii", 20, 0, 0, 0, 1)  # fWide=1 for Unicode
    data = header + file_bytes + b"\x00\x00"  # extra null terminator

    h_global = kernel32.GlobalAlloc(GHND, len(data))
    p_global = kernel32.GlobalLock(h_global)
    ctypes.memmove(p_global, data, len(data))
    kernel32.GlobalUnlock(h_global)

    user32.OpenClipboard(0)
    user32.EmptyClipboard()
    user32.SetClipboardData(CF_HDROP, h_global)
    user32.CloseClipboard()


def take_screenshot(capture_window=False):
    """Take a screenshot and copy the file to clipboard.

    Args:
        capture_window: If True, capture only the active window. If False, capture entire screen.

    Returns:
        The path to the saved screenshot file.
    """
    time.sleep(0.3)

    if capture_window:
        # Get the foreground window rect
        user32 = ctypes.windll.user32
        hwnd = user32.GetForegroundWindow()
        rect = ctypes.wintypes.RECT()
        user32.GetWindowRect(hwnd, ctypes.byref(rect))
        bbox = (rect.left, rect.top, rect.right, rect.bottom)
        img = ImageGrab.grab(bbox=bbox)
    else:
        img = ImageGrab.grab()

    temp_dir = os.path.join(tempfile.gettempdir(), "launchtype_screenshots")
    os.makedirs(temp_dir, exist_ok=True)

    timestamp = time.strftime("%Y%m%d_%H%M%S")
    file_path = os.path.join(temp_dir, f"screenshot_{timestamp}.jpg")
    img.save(file_path, "JPEG", quality=95)

    _copy_file_to_clipboard(file_path)

    return file_path


def get_screenshot_items():
    """Return the list of screenshot action items for the UI."""
    return [
        {
            "name": _("screenshot window"),
            "shortcut": "w",
            "type": "screenshot",
            "action": "window",
        },
        {
            "name": _("screenshot entire screen"),
            "shortcut": "s",
            "type": "screenshot",
            "action": "screen",
        },
    ]
