import json
from os.path import exists
from services.clipboard_history import ClipboardHistory
from helpers.plist_helper import parse_apple_snippets
from enums.ui_mode import UIMode
import uuid

import difflib

from helpers.sound_player import SoundPlayer
import os


class DataManager:
    commandsData = {}
    clipboard_history = ClipboardHistory()
    snippets = []

    def __init__(self, commands_file):
        self.commands_file = commands_file

        if not exists("snippets"):
            import os

            os.makedirs("snippets")

    def existsCommandsFile(self):
        return exists(self.commands_file)

    def create_commands_file(self):
        self.commandsData = {"commands": []}

        self.syncCommandsToStorage()

    def syncCommandsToStorage(self):
        with open(self.commands_file, "w") as outputFile:
            json_string = json.dumps(self.commandsData)

            outputFile.write(json_string)

    def loadCommandsFromFile(self):
        with open(self.commands_file, "r") as inputFile:
            self.commandsData = json.loads(inputFile.read())

    def add_command(self, command, name, args, abreviation):
        command_dictionary = {
            "path": command,
            "name": name.lower(),
            "args": args,
            "shortcut": abreviation.lower(),
            "id": str(uuid.uuid4()),
        }

        print(command_dictionary)

        self.commandsData["commands"].append(command_dictionary)

        self.syncCommandsToStorage()

    def get_data_list_items(self, search_string="", mode=UIMode.COMMANDS):
        if mode == UIMode.COMMANDS:
            return self.get_commands(search_string)

        if mode == UIMode.SNIPPETS:
            return self.get_snippets(search_string)

        if mode == UIMode.CLIPBOARD:
            return self.get_history_items(search_string)

    def get_commands_with_path(self, path):
        commands_to_return = []
        print(self)

        for command in self.commandsData["commands"]:
            if command["path"] == path:
                commands_to_return.append(command)

        return commands_to_return

    def get_commands(self, search_string=""):
        if search_string == "":
            return self.commandsData["commands"]

        # check if the string equals to any abreviation

        for command in self.commandsData["commands"]:
            if command["shortcut"] == search_string:
                SoundPlayer.play("match")

                return [command]

        # Find closest matching command strings containing the search string

        closest_matching__elements = difflib.get_close_matches(
            search_string,
            [command["name"] for command in self.commandsData["commands"]],
            cutoff=0.6,
        )

        # Return the commands associated with the elements

        # TODO: Is there a better way to do this?

        SoundPlayer.play("type")

        return [
            command
            for command in self.commandsData["commands"]
            if command["name"] in closest_matching__elements
        ]

        # If nothing found return empty array

        SoundPlayer.play("type")

        return []

    def pop_by_uuid(self, id):
        for command in self.commandsData["commands"]:
            if command["id"] == id:
                self.commandsData["commands"].remove(command)

                self.syncCommandsToStorage()
                return

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
        if search_string == "":
            return map(
                lambda snippet: {
                    "name": snippet["contents"],
                    "shortcut": snippet["shortcut"],
                    "type": "snippet",
                },
                self.snippets,
            )

        # check if the string equals to any abreviation
        for snippet in self.snippets:
            if snippet["shortcut"] == search_string:
                SoundPlayer.play("match")

                return [
                    {
                        "name": snippet["contents"],
                        "shortcut": snippet["shortcut"],
                        "type": "snippet",
                    }
                ]

    def check_if_shortcut_already_in_commands(self, shortcut_string):
        for command in self.commandsData["commands"]:
            if not shortcut_string == "" and shortcut_string == command["shortcut"]:
                return True

        return False

    def get_history_items(self, search_string):
        clipboard_items = self.clipboard_history.get_history_items()

        if search_string == "":
            return clipboard_items

        # check if the string equals to any abreviation
        for item in clipboard_items:
            if item["shortcut"] == search_string:
                SoundPlayer.play("match")
                return [item]

        # Find closest matching command strings containing the search string

        closest_matching__elements = difflib.get_close_matches(
            search_string, [item["name"] for item in clipboard_items], cutoff=0.6
        )

        SoundPlayer.play("type")

        return [
            command
            for command in self.commandsData["commands"]
            if command["name"] in closest_matching__elements
        ]

        # If nothing found return empty array

        SoundPlayer.play("type")
        return []

    def add_snippet(self, name, contents):
        with open("snippets/" + name + ".txt", "w", encoding="utf-8") as outputFile:
            outputFile.write(contents)

            self.snippets.append({"shortcut": name.lower(), "contents": contents})

        self.load_snippets_from_files()

    def forget_clipboard(self):
        self.clipboard_history.forget_last_value()

    def delete_clipboard_history_item_by_text(self, text_of_item):
        self.clipboard_history.delete_clipboard_history_item_by_text(text_of_item)
