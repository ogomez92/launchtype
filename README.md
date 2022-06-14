# Launchtype

I wrote this app to quickly launch commands (applications) with or without command line arguments on Windows.

I have an app on my mac called [Launchbar](https://www.obdev.at/products/launchbar/index.html) which does this very efficiently, letting me run apps or websites by the use of small commands or abreviations.

I don't like having a cluttered desktop on Windows, and sometimes I have a lot of different websites to run with complicated URLs and I have to find the text file where I have them stored, copy the address to the browser, etc. this is now over.

This is a launcher that can be used with the press of ctrl+alt+space (maybe I will make it configurable later).

You can add commands via the UI, for example add chrome.exe using a URL as arguments to run a website, or add your favorite game as the path to directly run that game using a comand.

From the UI you can also copy existing commands, edit them, and delete.

The commands are stored in a commands.json file which is modifiable via any text editor that supports JSON formatting to make it readable.

## Usage

This app doesn't yet have an executable, so you will need to download [Python 3](www.python.org) and then execute the following commands running as administrator (wxpython needs it):

```bash
pip install -r requirements.txt
```

To use the app, simply run:

```bash
python src/main.py
```

If you find an issue installing or running the app, please let me know. I am still unfamiliar with Python distribution, so it's probably an error on my part.

Once you add a command using the Add button in the UI, in order to use it you can either:

1. Select it from the list
2. Type its shortcut (if any) in the input field of the UI.
3. Type enough letters in the command's display name for it to show up in the list and the screen reader to speak it.

## Known issues

Alt F4 closes the application and you need to run it again (will fix).
Workaround: Use control alt space to hide its Window, or launch a command. Launching a command makes the window go away.

The visual appearance of the ap might not be up to standards. I'm blind and cannot debug the interface.
Workaround: Open a PR and help me make it better ;)

## TODO

 1. Find a way to prevent alt f4 from closing the window.
 2. Find a way to play audio on windows.
 3. Ensure that requirements.txt is properly set up.
 4. Compile this into an executable for windows.
 5. Possibly tweak the search difflib method.
