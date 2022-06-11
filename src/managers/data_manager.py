import json
from os.path import exists
import uuid
import difflib


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
            "shortcut": abreviation.lower(),
            "id": str(uuid.uuid4())
        }
        print(command_dictionary)
        self.commandsData['commands'].append(command_dictionary)
        self.syncCommandsToStorage()

    def get_commands(self, search_string=""):
        if search_string == "":
            return self.commandsData['commands']

        # check if the string equals to any abreviation
        for command in self.commandsData['commands']:
            if command['shortcut'] == search_string:
                return [command]

        # Find closest matching command strings containing the search string
        closest_matching__elements = difflib.get_close_matches(search_string, [command['name'] for command in self.commandsData['commands']], cutoff = 0.6)

        # Return the commands associated with the elements
        # TODO: Is there a better way to do this?
        return [command for command in self.commandsData['commands'] if command['name'] in closest_matching__elements]

        # If nothing found return empty array
        return []
        
    def delete_by_uuid(self, id):
        for command in self.commandsData['commands']:
            if command['id'] == id:
                self.commandsData['commands'].remove(command)
                self.syncCommandsToStorage()
                return
