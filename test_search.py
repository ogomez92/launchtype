"""
Simple test script to verify the new search functionality.
"""

import sys
sys.path.insert(0, 'src')

from helpers.search_utility import subsequence_match, fuzzy_search, check_exact_shortcut_match


def test_subsequence_matching():
    print("Testing subsequence matching...")

    # Test case from user: "google website" should match "gwe" or "g w"
    test_cases = [
        ("gwe", "google website", True),
        ("g w", "google website", True),
        ("goog", "google website", True),
        ("web", "google website", True),
        ("xyz", "google website", False),
        ("", "google website", True),  # Empty search matches everything
    ]

    for search, target, expected in test_cases:
        is_match, score = subsequence_match(search, target)
        status = "PASS" if is_match == expected else "FAIL"
        print(f"{status} '{search}' in '{target}': {is_match} (score: {score})")

    print()


def test_fuzzy_search():
    print("Testing fuzzy search with commands...")

    # Sample commands
    commands = [
        {"name": "google website", "shortcut": "gw", "path": "chrome.exe"},
        {"name": "github repo", "shortcut": "gh", "path": "chrome.exe"},
        {"name": "notepad", "shortcut": "np", "path": "notepad.exe"},
        {"name": "visual studio code", "shortcut": "vs", "path": "code.exe"},
    ]

    # Test searches
    test_searches = [
        ("gwe", ["google website"]),
        ("g w", ["google website", "visual studio code"]),  # Both have g and w
        ("gh", ["github repo"]),
        ("vsc", ["visual studio code"]),
        ("note", ["notepad"]),
    ]

    for search, expected_names in test_searches:
        results = fuzzy_search(search, commands, lambda cmd: cmd["name"])
        result_names = [cmd["name"] for cmd in results]
        print(f"Search '{search}':")
        print(f"  Expected: {expected_names}")
        print(f"  Got:      {result_names}")
        print()


def test_exact_shortcut_match():
    print("Testing exact shortcut matching...")

    commands = [
        {"name": "google website", "shortcut": "gw", "path": "chrome.exe"},
        {"name": "github repo", "shortcut": "gh", "path": "chrome.exe"},
    ]

    # Test exact shortcut matches
    match1 = check_exact_shortcut_match("gw", commands)
    print(f"Exact match 'gw': {match1['name'] if match1 else 'None'}")

    match2 = check_exact_shortcut_match("gh", commands)
    print(f"Exact match 'gh': {match2['name'] if match2 else 'None'}")

    match3 = check_exact_shortcut_match("xyz", commands)
    print(f"Exact match 'xyz': {match3['name'] if match3 else 'None'}")

    print()


if __name__ == "__main__":
    print("=" * 60)
    print("Search Utility Tests")
    print("=" * 60)
    print()

    test_subsequence_matching()
    test_fuzzy_search()
    test_exact_shortcut_match()

    print("=" * 60)
    print("Tests complete!")
    print("=" * 60)
