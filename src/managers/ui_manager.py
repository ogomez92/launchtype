import wx
from ui.command_edition_dialog import CommandEditionDialog
from helpers.sound_player import SoundPlayer
from ui.add_snippet_dialog import AddSnippetDialog
from services.runner_service import run_command
from services.speech_service import SpeechService
from enums.ui_mode import UIMode
from utility_functions import copy_to_clipboard
import webbrowser


class UIManager:
    global _
    commands_in_ui = []
    mode = UIMode.COMMANDS

    def __init__(self, data):
        self.app = wx.App(False)
        self.frame = wx.Frame(None, -1, "Launchtype")
        self.panel = wx.Panel(self.frame, -1)
        self.dataManager = data

        sizer = wx.BoxSizer(wx.VERTICAL)

        editSizer = wx.BoxSizer(wx.HORIZONTAL)
        editLabel = wx.StaticText(self.panel, label=_("Input Field"))
        self.edit = wx.TextCtrl(self.panel)
        self.app.Bind(wx.EVT_TEXT, self.update_list, self.edit)
        editSizer.Add(editLabel)
        editSizer.Add(self.edit)
        sizer.Add(editSizer)

        self.list = wx.ListBox(self.panel, style=wx.LB_SINGLE)
        sizer.Add(self.list)

        buttonRowSizer = wx.BoxSizer(wx.HORIZONTAL)
        self.add_button = wx.Button(self.panel, wx.ID_ADD, _("&Add..."))
        self.app.Bind(wx.EVT_BUTTON, self.add_button_clicked, self.add_button)
        buttonRowSizer.Add(self.add_button)

        self.edit_button = wx.Button(self.panel, wx.ID_EDIT, _("&Edit..."))
        self.app.Bind(wx.EVT_BUTTON, self.editButtonClicked, self.edit_button)
        buttonRowSizer.Add(self.edit_button)

        self.copy_button = wx.Button(self.panel, wx.ID_COPY, _("&COPY..."))
        self.app.Bind(wx.EVT_BUTTON, self.copyButtonClicked, self.copy_button)
        buttonRowSizer.Add(self.copy_button)

        self.delete_button = wx.Button(self.panel, wx.ID_DELETE, _("&Delete"))
        self.app.Bind(wx.EVT_BUTTON, self.deleteButtonClicked, self.delete_button)
        buttonRowSizer.Add(self.delete_button)

        self.snippets_button = wx.Button(self.panel, 1234, _("Open &Snippets folder"))
        self.app.Bind(wx.EVT_BUTTON, self.snippets_button_clicked, self.snippets_button)
        buttonRowSizer.Add(self.snippets_button)

        self.new_snippet_button = wx.Button(self.panel, 12345, _("&New snipet"))
        self.app.Bind(
            wx.EVT_BUTTON, self.new_snippet_button_clicked, self.new_snippet_button
        )
        buttonRowSizer.Add(self.new_snippet_button)

        self.run_button = wx.Button(self.panel, wx.ID_OK, _("&Run"))
        self.app.Bind(wx.EVT_BUTTON, self.run_button_clicked, self.run_button)
        self.run_button.SetDefault()
        buttonRowSizer.Add(self.run_button)

        self.help_button = wx.Button(self.panel, wx.ID_HELP, _("&Help"))
        self.app.Bind(wx.EVT_BUTTON, self.openDocs, self.help_button)
        buttonRowSizer.Add(self.help_button)

        sizer.Add(buttonRowSizer)

        # hide frame when escape pressed
        self.app.Bind(wx.EVT_KEY_DOWN, self.on_key_down)

    def initialize_ui(self):
        self.app.MainLoop()

    def show_alert(self, title, text):
        dlg = wx.MessageDialog(None, text, title, wx.OK)
        dlg.ShowModal()
        dlg.Destroy()

    def show_question_dialog(self, title, text):
        dlg = wx.MessageDialog(None, text, title, wx.YES_NO | wx.ICON_QUESTION)
        result = dlg.ShowModal()
        dlg.Destroy()

        return result == wx.ID_YES

    def show_error(self, title, text):
        dlg = wx.MessageDialog(None, text, title, wx.OK | wx.ICON_ERROR)
        dlg.ShowModal()
        dlg.Destroy()

    def add_button_clicked(self, event):
        with CommandEditionDialog(self.frame, self.dataManager) as addDialog:
            addDialog.ShowModal()

        self.edit.Value = ""

        self.update_list()

    def editButtonClicked(self, event):
        selected_option_index = self.list.GetSelection()
        if selected_option_index < 0:
            return

        selected_option = self.commands_in_ui[selected_option_index]

        with CommandEditionDialog(
            self.frame, self.dataManager, selected_option
        ) as addDialog:
            addDialog.ShowModal()

        self.edit.Value = ""

        self.update_list()

    def copyButtonClicked(self, event):
        selected_option_index = self.list.GetSelection()
        if selected_option_index < 0:
            return

        selected_option = self.commands_in_ui[selected_option_index].copy()
        # Remove the display name and the shortcut because its a copy
        selected_option["name"] = ""
        selected_option["shortcut"] = ""

        with CommandEditionDialog(
            self.frame, self.dataManager, selected_option
        ) as addDialog:
            addDialog.ShowModal()

        self.edit.Value = ""

        self.update_list()

    def deleteButtonClicked(self, event):
        selected_option_index = self.list.GetSelection()

        if selected_option_index < 0:
            return

        selected_option = self.commands_in_ui[selected_option_index]

        self.dataManager.pop_by_uuid(selected_option["id"])

        self.update_list()

    def toggle_visibility(self):
        isVisible = self.frame.IsShown()

        if isVisible:
            self.frame.Hide()
            SoundPlayer.play("hide")

        else:
            self.frame.Show()
            SoundPlayer.play("show")
            self.frame.Raise()
            self.edit.SetFocus()
            self.edit.Value = ""
            self.mode = UIMode.COMMANDS
            self.update_list()

    def update_list(self, event=None):
        if self.edit.Value == "-":
            SpeechService.speak(_("snippet mode"))
            self.dataManager.load_snippets_from_files()
            self.mode = UIMode.SNIPPETS
            self.edit.Value = ""

        if self.edit.Value == "?":
            SpeechService.speak(_("Clipboard history mode"))
            self.mode = UIMode.CLIPBOARD
            self.edit.Value = ""

        if self.edit.Value == ".":
            SpeechService.speak(_("commands mode"))
            self.mode = UIMode.COMMANDS
            self.edit.Value = ""

        self.commands_in_ui = []
        self.list.Clear()

        for command in self.dataManager.get_data_list_items(
            self.edit.Value.lower(), self.mode
        ):
            self.commands_in_ui.append(command)
            command_list_string = command["name"][:40]

            if not command["shortcut"] == "":
                shortcut = command["shortcut"]
                command_list_string = command_list_string + f"({shortcut})"
            self.list.Append(command_list_string)

        # Select the first item of the list
        if self.list.GetCount() > 0:
            self.select_first()

            # If user has typed something in the edit field, speak the first result
            if not self.edit.Value == "":
                SpeechService.speak(self.list.GetString(0))

    def run_button_clicked(self, event):
        try:
            selected_option_index = self.list.GetSelection()
            if selected_option_index < 0:
                return
            selected_option = self.commands_in_ui[selected_option_index]
            print(selected_option)
            self.frame.Hide()

            if "type" not in selected_option:
                selected_option["type"] = "command"

            if selected_option["type"] == "command":
                selected_command = str(selected_option["path"])
                selected_args = str(selected_option["args"])
                run_command(selected_command, selected_args)

            if selected_option["type"] == "snippet":
                selected_snippet_text = str(selected_option["name"])
                copy_to_clipboard(selected_snippet_text)
                SoundPlayer.play("copy")

            if selected_option["type"] == "clip":
                self.dataManager.delete_clipboard_history_item_by_text(
                    selected_option["name"]
                )
                self.dataManager.forget_clipboard()
                SoundPlayer.play("copy")
                copy_to_clipboard(str(selected_option["name"]))

        except Exception as e:
            import traceback

            traceback.print_exc()
            self.show_error(
                "Oops...", _(f"Something went wrong while running your command: {e}")
            )

    def select_first(self):
        self.list.Select(0)

    def snippets_button_clicked(self, event):
        self.toggle_visibility()
        import os

        snippets_folder_location = os.path.join(os.getcwd(), "snippets")
        os.startfile(snippets_folder_location)

    def new_snippet_button_clicked(self, event):
        # show the add snippet dialog and print the results
        with AddSnippetDialog(self.frame, self.dataManager) as addDialog:
            addDialog.ShowModal()

        self.edit.Value = ""
        self.update_list()
        self.toggle_visibility()

    def on_key_down(self, event):
        if (
            event.GetKeyCode() == wx.WXK_ESCAPE
            or event.GetKeyCode() == wx.WXK_F4
            and event.AltDown()
        ):
            self.frame.Hide()
            return

        event.Skip()

    def openDocs(self, event):
        self.show_alert(
            _("information"), _("The documentation will now open in your web browser.")
        )
        try:
            webbrowser.open_new(
                "https://github.com/ogomez92/launchtype/blob/main/README.md"
            )
        except webbrowser.Error as e:
            self.show_alert(
                _("Documentation error"),
                _(f"There was an error opening the documentation: {e}")
            )
