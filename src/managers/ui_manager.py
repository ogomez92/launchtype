import wx
from ui.command_edition_dialog import CommandEditionDialog
from ui.settings_dialog import SettingsDialog
from helpers.sound_player import SoundPlayer
from ui.add_snippet_dialog import AddSnippetDialog
from ui.add_timer_dialog import AddTimerDialog
from ui.add_alarm_dialog import AddAlarmDialog
from ui.notebrook_credentials_dialog import NotebrookCredentialsDialog
from services import notebrook_service
from services import realtime_service
from services.runner_service import run_command
from services.screenshot_service import take_screenshot
from services.speech_service import SpeechService
from enums.ui_mode import UIMode
from utility_functions import copy_to_clipboard
import threading
import webbrowser


class UIManager:
    commands_in_ui = []
    mode = UIMode.COMMANDS
    # Notes are always posted to this channel (created on demand).
    NOTEBROOK_CHANNEL = "feeds"

    def __init__(self, data, settings_manager, cli_snippets_on_invoke=False, cli_quiet=False):
        self.app = wx.App(False)
        self.frame = wx.Frame(None, -1, "Launchtype")
        self.panel = wx.Panel(self.frame, -1)
        self.dataManager = data
        self.settings_manager = settings_manager
        self.cli_snippets_on_invoke = cli_snippets_on_invoke
        self.cli_quiet = cli_quiet

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

        self.copy_args_button = wx.Button(self.panel, 12346, _("Copy Args (Ctrl+C)"))
        self.app.Bind(wx.EVT_BUTTON, self.copy_args_clicked, self.copy_args_button)
        buttonRowSizer.Add(self.copy_args_button)

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

        self.settings_button = wx.Button(self.panel, wx.ID_PREFERENCES, _("Se&ttings..."))
        self.app.Bind(wx.EVT_BUTTON, self.settings_button_clicked, self.settings_button)
        buttonRowSizer.Add(self.settings_button)

        self.exit_button = wx.Button(self.panel, wx.ID_EXIT, _("E&xit"))
        self.app.Bind(wx.EVT_BUTTON, self.exit_app, self.exit_button)
        buttonRowSizer.Add(self.exit_button)

        sizer.Add(buttonRowSizer)

        # hide frame when escape pressed
        self.app.Bind(wx.EVT_KEY_DOWN, self.on_key_down)

    def initialize_ui(self):
        self.app.MainLoop()

    @property
    def snippets_on_invoke(self):
        return self.cli_snippets_on_invoke or self.settings_manager.get("snippets_on_invoke")

    def _effective_sounds_enabled(self):
        return self.settings_manager.get("enable_sounds") and not self.cli_quiet

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
        if self.mode == UIMode.TIMERS:
            with AddTimerDialog(self.frame, self.dataManager) as addDialog:
                addDialog.ShowModal()
        elif self.mode == UIMode.ALARMS:
            with AddAlarmDialog(self.frame, self.dataManager) as addDialog:
                addDialog.ShowModal()
        else:
            with CommandEditionDialog(self.frame, self.dataManager) as addDialog:
                addDialog.ShowModal()

        self.edit.Value = ""

        self.update_list()

    def editButtonClicked(self, event):
        selected_option_index = self.list.GetSelection()
        if selected_option_index < 0:
            return

        selected_option = self.commands_in_ui[selected_option_index]

        if selected_option.get("type") == "snippet":
            with AddSnippetDialog(
                self.frame, self.dataManager, selected_option
            ) as editDialog:
                editDialog.ShowModal()
        else:
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
            if self.snippets_on_invoke:
                self.mode = UIMode.SNIPPETS

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

        if self.edit.Value == ",":
            SpeechService.speak(_("Steam games mode"))
            self.dataManager.scan_steam_games()
            self.mode = UIMode.STEAM
            self.edit.Value = ""

        if self.edit.Value == "'":
            SpeechService.speak(_("screenshots mode"))
            self.mode = UIMode.SCREENSHOTS
            self.edit.Value = ""

        if self.edit.Value == "[":
            SpeechService.speak(_("timers mode"))
            self.mode = UIMode.TIMERS
            self.edit.Value = ""

        if self.edit.Value == "]":
            SpeechService.speak(_("alarms mode"))
            self.mode = UIMode.ALARMS
            self.edit.Value = ""

        if self.edit.Value == "#":
            SpeechService.speak(_("Notebrook new note mode, type your note and press enter"))
            self.mode = UIMode.NOTEBROOK
            self.edit.Value = ""

        if self.edit.Value == "+":
            SpeechService.speak(_("realtime data mode"))
            self.mode = UIMode.REALTIME
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
                result_count = self.list.GetCount()
                first_result = self.list.GetString(0)
                if result_count == 1:
                    # Single result - likely a shortcut match
                    SpeechService.speak(first_result)
                else:
                    # Multiple results - announce focused result, count, and navigation hint
                    SpeechService.speak(_("{}, {} search results shown, use tab and down arrow to access more results").format(first_result, result_count))

    def run_button_clicked(self, event):
        if self.mode == UIMode.NOTEBROOK:
            self.send_notebrook_note()
            return

        try:
            selected_option_index = self.list.GetSelection()
            if selected_option_index < 0:
                return
            selected_option = self.commands_in_ui[selected_option_index]
            print(selected_option)

            if "type" not in selected_option:
                selected_option["type"] = "command"

            # Timers and alarms are toggled in place; keep the window open so the
            # user can see the new state and keep managing them.
            if selected_option["type"] == "timer":
                enabled = self.dataManager.toggle_timer(selected_option["id"])
                SoundPlayer.play("match")
                state = _("started") if enabled else _("stopped")
                SpeechService.speak(_("Timer {state}").format(state=state))
                self.update_list()
                return

            if selected_option["type"] == "alarm":
                enabled = self.dataManager.toggle_alarm(selected_option["id"])
                SoundPlayer.play("match")
                state = _("on") if enabled else _("off")
                SpeechService.speak(_("Alarm {state}").format(state=state))
                self.update_list()
                return

            # Realtime lookups fetch in the background and announce the value
            # when it arrives; keep the window open so the user can query
            # several values in a row.
            if selected_option["type"] == "realtime":
                self.fetch_realtime_value(selected_option)
                return

            self.frame.Hide()

            if selected_option["type"] == "command":
                selected_command = str(selected_option["path"])
                selected_args = str(selected_option["args"])
                run_as_admin = selected_option.get("run_as_admin", False)
                run_command(selected_command, selected_args, run_as_admin=run_as_admin)

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

            if selected_option["type"] == "steam":
                appid = str(selected_option["appid"])
                webbrowser.open(f"steam://rungameid/{appid}")
                SoundPlayer.play("run")

            if selected_option["type"] == "screenshot":
                capture_window = selected_option["action"] == "window"
                take_screenshot(capture_window=capture_window)
                SoundPlayer.play("copy")

        except Exception as e:
            import traceback

            traceback.print_exc()
            self.show_error(
                "Oops...", _(f"Something went wrong while running your command: {e}")
            )

    def fetch_realtime_value(self, item):
        SoundPlayer.play("run")
        SpeechService.speak(_("Fetching {name}").format(name=item["name"]))

        def worker():
            try:
                announcement = realtime_service.fetch_value(item["key"])
            except Exception as error:  # noqa: BLE001 - always announce the failure
                wx.CallAfter(
                    self.announce_realtime_result,
                    _("Could not fetch {name}: {reason}").format(
                        name=item["name"], reason=error
                    ),
                    False,
                )
                return

            wx.CallAfter(self.announce_realtime_result, announcement, True)

        threading.Thread(target=worker, daemon=True).start()

    def announce_realtime_result(self, announcement, success):
        if success:
            SoundPlayer.play("match")
        SpeechService.speak(announcement)

    def _ensure_notebrook_credentials(self):
        """Return (url, token), prompting once and saving them if not set.

        Returns (None, None) if the user cancels the credentials dialog.
        """
        url = self.settings_manager.get("notebrook_url")
        token = self.settings_manager.get("notebrook_token")

        if url and token:
            return url, token

        with NotebrookCredentialsDialog(self.frame, url or "", token or "") as dlg:
            if dlg.ShowModal() != wx.ID_OK:
                return None, None
            url, token = dlg.url, dlg.token

        self.settings_manager.set("notebrook_url", url)
        self.settings_manager.set("notebrook_token", token)
        self.settings_manager.save()
        return url, token

    def send_notebrook_note(self):
        note = self.edit.Value.strip()
        # Don't run if no text was entered.
        if not note:
            SpeechService.speak(_("No note entered"))
            return

        url, token = self._ensure_notebrook_credentials()
        if not url or not token:
            return

        try:
            notebrook_service.send_note(url, token, self.NOTEBROOK_CHANNEL, note)
        except notebrook_service.NotebrookError as e:
            if e.unauthorized:
                # Forget the rejected credentials so we ask again next time.
                self.settings_manager.set("notebrook_url", "")
                self.settings_manager.set("notebrook_token", "")
                self.settings_manager.save()
            SpeechService.speak(_("Note not sent"))
            self.show_error(_("Note not sent"), str(e))
            return

        SoundPlayer.play("run")
        SpeechService.speak(
            _("Note sent to {}").format(self.NOTEBROOK_CHANNEL)
        )
        self.mode = UIMode.COMMANDS
        self.edit.Value = ""
        self.frame.Hide()

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

    def copy_args_clicked(self, event):
        selected_option_index = self.list.GetSelection()
        if selected_option_index < 0:
            return

        selected_option = self.commands_in_ui[selected_option_index]
        args = selected_option.get("args", "")
        if args:
            copy_to_clipboard(args)
            SoundPlayer.play("copy")
            SpeechService.speak(_("Arguments copied"))
        else:
            SpeechService.speak(_("No arguments"))

    def on_key_down(self, event):
        if (
            event.GetKeyCode() == wx.WXK_ESCAPE
            or event.GetKeyCode() == wx.WXK_F4
            and event.AltDown()
        ):
            self.frame.Hide()
            return

        if event.GetKeyCode() == ord('C') and event.ControlDown():
            self.copy_args_clicked(event)
            return

        event.Skip()

    def openDocs(self, event):
        self.show_alert(
            _("information"), _("The documentation will now open in your web browser.")
        )
        try:
            webbrowser.open_new(
                _("https://github.com/ogomez92/launchtype/blob/main/README.md")
            )
        except webbrowser.Error as e:
            self.show_alert(
                _("Documentation error"),
                _(f"There was an error opening the documentation: {e}")
            )

    def settings_button_clicked(self, event):
        with SettingsDialog(self.frame, self.settings_manager) as dlg:
            dlg.ShowModal()
        SoundPlayer.enabled = self._effective_sounds_enabled()

    def exit_app(self, event):
        # Stop the clipboard history background thread before exiting
        self.dataManager.clipboard_history.stop()
        self.dataManager.timer_service.stop()
        self.dataManager.alarm_service.stop()
        self.frame.Destroy()
        self.app.ExitMainLoop()
