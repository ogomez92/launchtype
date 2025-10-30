# Launchtype

I wrote this app to quickly launch commands (applications) with or without command line arguments on Windows.

I have an app on my mac called [Launchbar](https://www.obdev.at/products/launchbar/index.html) which does this very efficiently, letting me run apps or websites by the use of small commands or abreviations.

I don't like having a cluttered desktop on Windows, and sometimes I have a lot of different websites to run with complicated URLs and I have to find the text file where I have them stored, copy the address to the browser, etc. this is now over.

This is a launcher that can be used with the press of ctrl+alt+space (maybe I will make it configurable later).

You can add commands via the UI, for example add chrome.exe using a URL as arguments to run a website, or add your favorite game as the path to directly run that game using a comand.

From the UI you can also copy existing commands, edit them, and delete.

The commands are stored in a commands.json file (or any other file you specify in the command line). which is modifiable via any text editor that supports JSON formatting to make it readable.

## Usage

The app includes some comand line parameters, mainly -m to start minimized and -c [filename] to specify a different commands file.

Once you add a command using the Add button in the UI, in order to use it you can either:

1. Select it from the list
2. Type its shortcut (if any) in the input field of the UI.
3. Type enough letters in the command's display name for it to show up in the list and the screen reader to speak it.

## Snippets

Snippets are pieces of text that, when their filename is typed to the input field of the UI, the content  of the file is put in the clipboard.

In order to use snippets, you need to create .txt files in the snippets folder of the app.

The name of the file is its shortcut, without the txt extension, and the content is what gets coppied.

For example, if you have a file called email.txt in the snippets folder which contains the text my_email@gmail.com, whenever you type "email" in the input field of the app and select it from the list by pressing enter, your email will be in the clipboard, my_email@gmail.com.

In order to access snippets you need to be in snippets mode, you can do this by typing a dash character (-) in the input field. This will cause all the commands to be removed form the list and the snippets will show up.

To go back to commands mode, you can press the period key (.). anyway, each time the app is opened by using ctrl alt space, it is by default in commands mode so nothing needs to be done.

## Clipboard History

Clipboard history can be accessed by pressing ? (question mark) in the input field. It will show up to 50 text items that you coppied to your clipboard, and it persists across restarts.

IT will only work with textual items, not file paths or stuff like that.

## Known issues

Alt F4 closes the application and you need to run it again (will fix).
Workaround: Use control alt space to hide its Window, or launch a command. Launching a command makes the window go away.

The visual appearance of the ap might not be up to standards. I'm blind and cannot debug the interface.
Workaround: Open a PR and help me make it better ;)

## TODO

 1. Find a way to prevent alt f4 from closing the window.
 2. Find a way to play audio on windows.
 3. Ensure that pyproject.toml dependencies are properly set up.
 4. Compile this into an executable for windows.
 5. Possibly tweak the search difflib method.
 6. Refactor command and snippet handling in the UI
