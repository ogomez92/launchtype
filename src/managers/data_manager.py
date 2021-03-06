import json
from os.path import exists
from enums.ui_mode import UIMode
import uuid
import difflib
from helpers.sound_player import SoundPlayer
import os

class DataManager:
    commandsData = {}
    snippets = []

    def __init__(self):
        if not exists('snippets'):
            import os
            os.makedirs('snippets')

    def existsCommandsFile(self):
        return exists('commands.json')

    def create_commands_file(self):
        self.commandsData = {
            'commands': []
        }
        self.syncCommandsToStorage()

    def syncCommandsToStorage(self):
        with open('commands.json', 'w') as outputFile:
            json_string = json.dumps(self.commandsData)
            outputFile.write(json_string)

    def loadCommandsFromFile(self):
        with open('commands.json', 'r') as inputFile:
            self.commandsData = json.loads(inputFile.read())

    def add_command(self, command, name, args, abreviation):
        command_dictionary = {
            "path": command,
            "name": name.lower(),
            "args": args,
            "shortcut": abreviation.lower(),
            "id": str(uuid.uuid4())
        }
        print(command_dictionary)
        self.commandsData['commands'].append(command_dictionary)
        self.syncCommandsToStorage()

    def get_data_list_items(self, search_string="", mode = UIMode.COMMANDS):
        if mode == UIMode.COMMANDS:
            return self.get_commands(search_string)

        if mode == UIMode.SNIPPETS:
            return self.get_snippets(search_string)

    def get_commands(self, search_string = ''):
        if search_string == "":
            return self.commandsData['commands']

        # check if the string equals to any abreviation
        for command in self.commandsData['commands']:
            if command['shortcut'] == search_string:
                SoundPlayer.play("match")
                return [command]

        # Find closest matching command strings containing the search string
        closest_matching__elements = difflib.get_close_matches(search_string, [command['name'] for command in self.commandsData['commands']], cutoff = 0.6)

        # Return the commands associated with the elements
        # TODO: Is there a better way to do this?
        SoundPlayer.play("type")
        return [command for command in self.commandsData['commands'] if command['name'] in closest_matching__elements]

        # If nothing found return empty array
        SoundPlayer.play("type")
        return []

    def delete_by_uuid(self, id):
        for command in self.commandsData['commands']:
            if command['id'] == id:
                self.commandsData['commands'].remove(command)
                self.syncCommandsToStorage()
                return

    def load_snippets_from_files(self):
        self.snippets = []
        for file in os.listdir('snippets'):
            file_without_extension = file.split('.')[0]
            with open('snippets/' + file, 'r') as inputFile:
                self.snippets.append({
                    'shortcut': file_without_extension.lower(),
                    'contents': inputFile.read()
                    })
                    
    def get_snippets(self, search_string):
        if search_string == "":
            return map(lambda snippet: {
                'name': snippet['contents'],
                'shortcut': snippet['shortcut'],
                'type': 'snippet'
                }, self.snippets)

        # check if the string equals to any abreviation
        for snippet in self.snippets:
            if snippet['shortcut'] == search_string:
                SoundPlayer.play("match")
                return [{
                    "name": snippet['contents'],
                    "shortcut": snippet['shortcut'],
                    "type": 'snippet'
                }]

    def check_if_shortcut_already_in_commands(self, shortcut_string):
        for command in self.commandsData['commands']:
            if not shortcut_string == "" and shortcut_string == command['shortcut']:
                return True

        return False