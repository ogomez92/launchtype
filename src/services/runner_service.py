import subprocess
import os
from helpers.sound_player import SoundPlayer


def run_command(path, args):
    comma_separated_args = None
    print('args', args)
    if args is not '':
        # separate args by comma

        comma_separated_args = [arg.strip() for arg in args.split(',')]

    cwd = os.path.dirname(path)
    SoundPlayer.play("run")
    command_to_run = [path]
    if comma_separated_args is not None:
        command_to_run.extend(comma_separated_args)
    subprocess.Popen(command_to_run, cwd=cwd)
