import subprocess


def run_command(path, args):
    subprocess.Popen([path, args])
