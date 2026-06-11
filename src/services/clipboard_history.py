import pyperclip
import threading
import time
import uuid
import json

from helpers.json_storage import atomic_write_json

HISTORY_FILE = "clipboard_history.json"
MAX_ITEMS = 50


class ClipboardHistory:
    def __init__(self):
        self.last_value = None
        self.history_items = []
        self._lock = threading.Lock()
        self.load_history_from_file()

        self._stop_event = threading.Event()
        self._thread = threading.Thread(target=self._watch, daemon=True)
        self._thread.start()

    def stop(self):
        self._stop_event.set()
        self._thread.join(timeout=2)
        self.history_items = []

    def _watch(self):
        # The loop must survive any single failed tick: pyperclip raises when
        # another process holds the clipboard lock, and the storage file can be
        # momentarily locked by scanners/sync tools. Skip the tick and retry.
        while not self._stop_event.is_set():
            time.sleep(0.1)

            try:
                value = pyperclip.paste()

                if value and value != self.last_value:
                    self.add_item_to_history(value)
                    self.last_value = value
            except Exception:
                continue

    def add_item_to_history(self, value):
        with self._lock:
            self.history_items = [
                item for item in self.history_items if item != value
            ]
            self.history_items.insert(0, value)

            if len(self.history_items) > MAX_ITEMS:
                self.history_items = self.history_items[:MAX_ITEMS]

            self._sync_to_storage_locked()

    def get_history_items(self):
        # create list with history_items with its name as value and index as shortcut
        with self._lock:
            snapshot = list(self.history_items)

        history_items = []
        for index, item in enumerate(snapshot):
            history_items.append(
                {
                    "name": item,
                    "shortcut": str(index + 1),
                    "id": str(uuid.uuid4()),
                    "type": "clip",
                }
            )

        return history_items

    def _sync_to_storage_locked(self):
        # A locked file just skips this sync; the next one rewrites the full
        # list anyway.
        try:
            atomic_write_json(HISTORY_FILE, self.history_items)
        except OSError:
            pass

    def load_history_from_file(self):
        try:
            with open(HISTORY_FILE, "r", encoding="utf-8") as inputFile:
                items = json.loads(inputFile.read())
            if isinstance(items, list):
                self.history_items = [
                    item for item in items if isinstance(item, str)
                ]
        except (OSError, ValueError):
            self.history_items = []

    def forget_last_value(self):
        self.last_value = None

    def delete_clipboard_history_item_by_text(self, text):
        with self._lock:
            self.history_items = [
                item for item in self.history_items if item != text
            ]
            self._sync_to_storage_locked()
