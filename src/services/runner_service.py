import subprocess
import os
import ctypes
from helpers.sound_player import SoundPlayer


def run_command(path, args, run_as_admin=False):
    comma_separated_args = None
    print("args", args)
    if args != "":
        # separate args by comma

        comma_separated_args = [arg.strip() for arg in args.split(",")]

    cwd = os.path.dirname(path)
    SoundPlayer.play("run")
    params = " ".join(comma_separated_args) if comma_separated_args else ""

    if run_as_admin:
        ctypes.windll.shell32.ShellExecuteW(None, "runas", path, params, cwd, 1)
    else:
        command_to_run = [path]
        if comma_separated_args is not None:
            command_to_run.extend(comma_separated_args)
        try:
            subprocess.Popen(command_to_run, cwd=cwd)
        except OSError as e:
            if e.winerror == 740:
                ctypes.windll.shell32.ShellExecuteW(None, "runas", path, params, cwd, 1)
            else:
                raise
