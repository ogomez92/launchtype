import pyperclip
import threading
import time
import uuid
import json

class ClipboardHistory:
    def __init__(self):
        self.last_value = None
        self.history_items = []
        self.createClipboardHistoryFileIFNotExists()
        self.load_history_from_file()

        self._stop_event = threading.Event()
        self._thread = threading.Thread(target=self._watch)
        self._thread.start()

    def stop(self):
        self._stop_event.set()
        self._thread.join()
        self.history_items = []

    def _watch(self):
        while not self._stop_event.is_set():
            time.sleep(0.1)

            value = pyperclip.paste()
            
            if value != "" and value != self.last_value:
                self.add_item_to_history(value)
                self.last_value = value

    def add_item_to_history(self, value):
        if value not in self.history_items:
            self.history_items.insert(0, value)
            
        if len(self.history_items) > 50:
            self.history_items.pop()

        self.sync_to_storage()

    def get_history_items(self):
        # create list with history_items with its name as value and index as shortcut
        history_items = []
        for index, item in enumerate(self.history_items):
            history_items.append({
                "name": item,
                "shortcut": str(index + 1),
                "id": str(uuid.uuid4()),
                "type": "clip"
                })
        
        return history_items

    def sync_to_storage(self):
        with open('clipboard_history.json', 'w') as outputFile:

            json_string = json.dumps(self.history_items)

            outputFile.write(json_string)

    def load_history_from_file(self):
        with open('clipboard_history.json', 'r') as inputFile:

            self.history_items = json.loads(inputFile.read())

    def createClipboardHistoryFileIFNotExists(self):
        try:
            with open('clipboard_history.json', 'r') as inputFile:
                pass
        except FileNotFoundError:
            with open('clipboard_history.json', 'w') as outputFile:
                outputFile.write("[]")

    def forget_last_value(self):
        self.last_value = None

    def delete_clipboard_history_item_by_text(self, text):
        for index, item in enumerate(self.history_items):
            if item == text:
                self.history_items.pop(index)

        self.sync_to_storage()
