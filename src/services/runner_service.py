import subprocess
import os
from helpers.sound_player import SoundPlayer


def run_command(path, args):
    # separate args by comma

    comma_separated_args = [arg.strip() for arg in args.split(',')]
    cwd = os.path.dirname(path)
    SoundPlayer.play("run")
    command_to_run = [path]
    command_to_run.extend(comma_separated_args)
    print(command_to_run)
    subprocess.Popen(command_to_run, cwd=cwd)
