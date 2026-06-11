import json
import os


def atomic_write_json(path, data, indent=None):
    """Persist JSON by writing a temp file and swapping it in, so a process
    killed mid-write can never leave a truncated/corrupt file behind."""
    temp_path = path + ".tmp"
    with open(temp_path, "w", encoding="utf-8") as outputFile:
        outputFile.write(json.dumps(data, indent=indent))
    os.replace(temp_path, path)
