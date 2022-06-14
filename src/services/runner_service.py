import subprocess
import os
from helpers.sound_player import SoundPlayer


def run_command(path, args):
    cwd = os.path.dirname(path)
    SoundPlayer.play("run")
    subprocess.Popen([path, args], cwd=cwd)
