import os
import re
import uuid


class SteamScanner:
    def __init__(self, library_path=None):
        self.library_path = library_path or r"C:\Program Files (x86)\Steam\steamapps"
        self.games = []

    def scan_games(self):
        """Scan Steam library for installed games by parsing appmanifest files."""
        self.games = []

        if not os.path.exists(self.library_path):
            return self.games

        try:
            for filename in os.listdir(self.library_path):
                if filename.startswith("appmanifest_") and filename.endswith(".acf"):
                    filepath = os.path.join(self.library_path, filename)
                    game = self._parse_appmanifest(filepath)
                    if game:
                        self.games.append(game)
        except OSError:
            pass

        # Sort games alphabetically by name
        self.games.sort(key=lambda g: g["name"].lower())
        return self.games

    def _parse_appmanifest(self, filepath):
        """Parse a single appmanifest ACF file to extract game info."""
        try:
            with open(filepath, "r", encoding="utf-8") as f:
                content = f.read()

            appid_match = re.search(r'"appid"\s+"(\d+)"', content)
            name_match = re.search(r'"name"\s+"([^"]+)"', content)

            if appid_match and name_match:
                appid = appid_match.group(1)
                name = name_match.group(1)
                return {
                    "name": name.lower(),
                    "shortcut": "",
                    "id": str(uuid.uuid4()),
                    "appid": appid,
                    "type": "steam",
                }
        except (OSError, UnicodeDecodeError):
            pass

        return None

    def get_games_as_items(self):
        """Return games in UI format."""
        if not self.games:
            self.scan_games()
        return self.games
