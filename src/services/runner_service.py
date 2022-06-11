import subprocess
import os


def run_command(path, args):
    cwd = os.path.dirname(path)
    subprocess.Popen([path, args], cwd=cwd)
