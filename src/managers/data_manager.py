import json
from os.path import exists
from services.clipboard_history import ClipboardHistory
from services.steam_scanner import SteamScanner
from services.screenshot_service import get_screenshot_items
from services import realtime_service
from services.timer_service import TimerService
from services.alarm_service import AlarmService
from helpers.json_storage import atomic_write_json
from helpers.plist_helper import parse_apple_snippets
from helpers.search_utility import fuzzy_search, check_exact_shortcut_match
from enums.ui_mode import UIMode
import uuid

from helpers.sound_player import SoundPlayer
import os


class DataManager:
    commandsData = {}
    clipboard_history = ClipboardHistory()
    snippets = []

    def __init__(self, commands_file, steam_library_path=None):
        self.commands_file = commands_file
        self.steam_scanner = SteamScanner(steam_library_path)
        self.timer_service = TimerService()
        self.alarm_service = AlarmService()

        if not exists("snippets"):
            import os

            os.makedirs("snippets")

    def existsCommandsFile(self):
        return exists(self.commands_file)

    def create_commands_file(self):
        self.commandsData = {"commands": []}

        self.syncCommandsToStorage()

    def syncCommandsToStorage(self):
        atomic_write_json(self.commands_file, self.commandsData)

    def loadCommandsFromFile(self):
        # A corrupt or malformed commands file must not prevent the app from
        # starting: move it aside so the data stays recoverable, then start
        # over with an empty command list.
        try:
            with open(self.commands_file, "r", encoding="utf-8") as inputFile:
                data = json.loads(inputFile.read())
            if not isinstance(data, dict) or not isinstance(
                data.get("commands"), list
            ):
                raise ValueError("commands file has an unexpected shape")
            self.commandsData = data
        except (OSError, ValueError):
            try:
                os.replace(self.commands_file, self.commands_file + ".corrupt")
            except OSError:
                pass
            self.create_commands_file()

    def add_command(self, command, name, args, abreviation, run_as_admin=False):
        command_dictionary = {
            "path": command,
            "name": name.lower(),
            "args": args,
            "shortcut": abreviation.lower(),
            "id": str(uuid.uuid4()),
            "run_as_admin": run_as_admin,
        }

        self.commandsData["commands"].append(command_dictionary)

        self.syncCommandsToStorage()

    def get_data_list_items(self, search_string="", mode=UIMode.COMMANDS):
        if mode == UIMode.COMMANDS:
            return self.get_commands(search_string)

        if mode == UIMode.SNIPPETS:
            return self.get_snippets(search_string)

        if mode == UIMode.CLIPBOARD:
            return self.get_history_items(search_string)

        if mode == UIMode.STEAM:
            return self.get_steam_games(search_string)

        if mode == UIMode.SCREENSHOTS:
            return get_screenshot_items()

        if mode == UIMode.TIMERS:
            return self.get_timers(search_string)

        if mode == UIMode.ALARMS:
            return self.get_alarms(search_string)

        if mode == UIMode.NOTEBROOK:
            # The note content is taken straight from the edit field on run,
            # so there is nothing to list here.
            return []

        if mode == UIMode.REALTIME:
            return self.get_realtime_items(search_string)

    def get_commands_with_path(self, path):
        commands_to_return = []

        for command in self.commandsData["commands"]:
            if command["path"] == path:
                commands_to_return.append(command)

        return commands_to_return

    def get_commands(self, search_string=""):
        if search_string == "":
            return self.commandsData["commands"]

        # Check if the string equals any shortcut (exact match has priority)
        exact_match = check_exact_shortcut_match(
            search_string, self.commandsData["commands"], "shortcut"
        )
        if exact_match:
            SoundPlayer.play("match")
            return [exact_match]

        # Use fuzzy subsequence search on command names
        results = fuzzy_search(
            search_string, self.commandsData["commands"], lambda cmd: cmd["name"]
        )

        if results:
            SoundPlayer.play("type")
        else:
            SoundPlayer.play("type")

        return results

    def pop_by_uuid(self, id):
        for command in self.commandsData["commands"]:
            if command["id"] == id:
                self.commandsData["commands"].remove(command)

                self.syncCommandsToStorage()
                return

        # The id may belong to a timer or an alarm instead of a command.
        self.timer_service.remove(id)
        self.alarm_service.remove(id)

    def load_snippets_from_files(self):
        self.snippets = []

        apple_snippets = parse_apple_snippets("apple_snippets.plist")

        if apple_snippets:
            self.snippets.extend(apple_snippets)

        for file in os.listdir("snippets"):
            file_without_extension = file.split(".")[0]

            with open("snippets/" + file, "r", encoding="utf-8") as inputFile:
                self.snippets.append(
                    {
                        "shortcut": file_without_extension.lower(),
                        "contents": inputFile.read(),
                    }
                )

    def get_snippets(self, search_string):
        # Convert snippets to display format
        formatted_snippets = [
            {
                "name": snippet["contents"],
                "shortcut": snippet["shortcut"],
                "type": "snippet",
            }
            for snippet in self.snippets
        ]

        if search_string == "":
            return formatted_snippets

        # Check if the string equals any shortcut (exact match has priority)
        exact_match = check_exact_shortcut_match(
            search_string, formatted_snippets, "shortcut"
        )
        if exact_match:
            SoundPlayer.play("match")
            return [exact_match]

        # Use fuzzy subsequence search on snippet shortcuts and contents
        results = fuzzy_search(
            search_string,
            formatted_snippets,
            lambda snip: f"{snip['shortcut']} {snip['name']}",
        )

        if results:
            SoundPlayer.play("type")
        else:
            SoundPlayer.play("type")

        return results

    def check_if_shortcut_already_in_commands(self, shortcut_string):
        for command in self.commandsData["commands"]:
            if not shortcut_string == "" and shortcut_string == command["shortcut"]:
                return True

        return False

    def get_history_items(self, search_string):
        clipboard_items = self.clipboard_history.get_history_items()

        if search_string == "":
            return clipboard_items

        # Check if the string equals any shortcut (exact match has priority)
        exact_match = check_exact_shortcut_match(
            search_string, clipboard_items, "shortcut"
        )
        if exact_match:
            SoundPlayer.play("match")
            return [exact_match]

        # Use fuzzy subsequence search on clipboard item text
        results = fuzzy_search(
            search_string, clipboard_items, lambda item: item["name"]
        )

        if results:
            SoundPlayer.play("type")
        else:
            SoundPlayer.play("type")

        return results

    def scan_steam_games(self):
        """Trigger a scan of the Steam library."""
        self.steam_scanner.scan_games()

    def get_steam_games(self, search_string):
        """Get Steam games, optionally filtered by search string."""
        steam_games = self.steam_scanner.get_games_as_items()

        if search_string == "":
            return steam_games

        # Use fuzzy subsequence search on game names
        results = fuzzy_search(
            search_string, steam_games, lambda game: game["name"]
        )

        if results:
            SoundPlayer.play("type")
        else:
            SoundPlayer.play("type")

        return results

    def get_timers(self, search_string):
        timers = self.timer_service.get_items()

        if search_string == "":
            return timers

        results = fuzzy_search(search_string, timers, lambda timer: timer["name"])
        SoundPlayer.play("type")
        return results

    def add_timer(self, title, description, minutes, repeating, sound):
        self.timer_service.add_timer(title, description, minutes, repeating, sound)

    def toggle_timer(self, timer_id):
        return self.timer_service.toggle(timer_id)

    def get_alarms(self, search_string):
        alarms = self.alarm_service.get_items()

        if search_string == "":
            return alarms

        results = fuzzy_search(search_string, alarms, lambda alarm: alarm["name"])
        SoundPlayer.play("type")
        return results

    def get_realtime_items(self, search_string):
        items = realtime_service.get_realtime_items()

        if search_string == "":
            return items

        # Check if the string equals any shortcut (exact match has priority)
        exact_match = check_exact_shortcut_match(search_string, items, "shortcut")
        if exact_match:
            SoundPlayer.play("match")
            return [exact_match]

        results = fuzzy_search(search_string, items, lambda item: item["name"])
        SoundPlayer.play("type")
        return results

    def add_alarm(self, title, description, hour, minute, sound):
        self.alarm_service.add_alarm(title, description, hour, minute, sound)

    def toggle_alarm(self, alarm_id):
        return self.alarm_service.toggle(alarm_id)

    def add_snippet(self, name, contents):
        with open("snippets/" + name + ".txt", "w", encoding="utf-8") as outputFile:
            outputFile.write(contents)

            self.snippets.append({"shortcut": name.lower(), "contents": contents})

        self.load_snippets_from_files()

    def update_snippet(self, original_shortcut, name, contents):
        # Remove the old file if the snippet was renamed so we don't leave a
        # stale duplicate behind.
        if original_shortcut and original_shortcut.lower() != name.lower():
            old_path = "snippets/" + original_shortcut + ".txt"
            if exists(old_path):
                os.remove(old_path)

        with open("snippets/" + name + ".txt", "w", encoding="utf-8") as outputFile:
            outputFile.write(contents)

        self.load_snippets_from_files()

    def forget_clipboard(self):
        self.clipboard_history.forget_last_value()

    def delete_clipboard_history_item_by_text(self, text_of_item):
        self.clipboard_history.delete_clipboard_history_item_by_text(text_of_item)
