import json
from os.path import exists

from helpers.json_storage import atomic_write_json


DEFAULT_STEAM_LIBRARY = r"C:\Program Files (x86)\Steam\steamapps"

DEFAULTS = {
    "enable_sounds": True,
    "start_minimized": False,
    "snippets_on_invoke": False,
    "steam_library": DEFAULT_STEAM_LIBRARY,
    # Notebrook credentials. Stored locally only (settings.json is gitignored),
    # never committed to the repository.
    "notebrook_url": "",
    "notebrook_token": "",
}


class SettingsManager:
    def __init__(self, settings_file="settings.json"):
        self.settings_file = settings_file
        self.settings = dict(DEFAULTS)
        self.load()

    def load(self):
        if not exists(self.settings_file):
            return
        try:
            with open(self.settings_file, "r", encoding="utf-8") as f:
                loaded = json.loads(f.read())
            for key in DEFAULTS:
                if key in loaded:
                    self.settings[key] = loaded[key]
        except (OSError, ValueError):
            pass

    def save(self):
        atomic_write_json(self.settings_file, self.settings, indent=2)

    def get(self, key):
        return self.settings.get(key, DEFAULTS.get(key))

    def set(self, key, value):
        self.settings[key] = value

    def update(self, values):
        for key, value in values.items():
            if key in DEFAULTS:
                self.settings[key] = value
