import json
from os.path import exists


class DataManager:
    commandsData = {}

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
            "shortcut": abreviation.lower()
        }

        self.commandsData['commands'].append(command_dictionary)
        self.syncCommandsToStorage()

    def get_commands(self, search_string=""):
        if search_string == "":
            return self.commandsData['commands']

        