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

    def loadCommandsFromFile():
        with open('commands.json', 'r') as inputFile:
            self.commandsData = json.loads(inputFile)
