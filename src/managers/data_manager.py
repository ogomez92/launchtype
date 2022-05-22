import json
from os.path import exists


class DataManager:
    commandsData = {}

    def existsCommandsFile(self):
        return exists('commands.json')

    def loadCommandsFile(self):
        if not self.existsCommandsFile():
            self.commandsData = {
                'commands': []
            }
            self.syncCommandsToStorage()
        else:
            self.loadCommandsFromFile()

    def syncCommandsToStorage(self):
        with open('commands.json', 'w') as outputFile:
            json.dumps((self.commandsData), outputFile)

    def loadCommandsFromFile():
        with open('commands.json', 'r') as inputFile:
            self.commandsData = json.loads(inputFile)
