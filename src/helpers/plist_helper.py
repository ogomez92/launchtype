import plistlib
import os


def parse_apple_snippets(file_path):
    # Check if file_path exists
    if not os.path.exists(file_path):
        return []

    with open(file_path, "rb") as file:
        plist_data = plistlib.load(file)

    entries = []
    for item in plist_data:
        # if shortcut begins with dash, strip it
        if item["shortcut"].startswith("-"):
            item["shortcut"] = item["shortcut"][1:]

        entry = {
            "shortcut": item["shortcut"],
            "contents": item["phrase"],
        }
        entries.append(entry)

    return entries
