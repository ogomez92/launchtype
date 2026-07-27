#!/usr/bin/env python3
"""Regenerate the emoji table compiled into the binary.

Emoji names are the ones screen readers use, and neither Windows nor macOS
exposes them through an API we can call: Windows ships no emoji-name table at
all, and the macOS one lives inside a private framework. So the table is baked
into the app instead, from the same CLDR annotation data every emoji picker
uses. Nothing is fetched at build time or at run time — only this script talks
to the network, and only when someone wants a newer Unicode release.

Run from the repo root:  python scripts/gen_emoji.py

Which emoji exist, and the order they are listed in, come from Unicode's
emoji-test.txt (the RGI set, already in the palette order every emoji keyboard
uses). The names and keywords come from CLDR, one column pair per language.

The output is one tab-separated line per emoji:

    <emoji>\t<English name>\t<English keywords>\t<Spanish name>\t<Spanish keywords>

Keywords are space separated; the app searches name + keywords together and
displays the name alone.
"""
import re
import sys
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT_PATH = ROOT / "crates" / "launchtype-core" / "data" / "emoji.txt"

# Both pinned so a regeneration is reproducible; bump for a newer Unicode
# release (CLDR 48 carries the annotations for Unicode 16.0 emoji).
CLDR = "release-48"
EMOJI = "16.0"
CLDR_BASE = f"https://raw.githubusercontent.com/unicode-org/cldr/{CLDR}/common"
EMOJI_TEST = f"https://unicode.org/Public/emoji/{EMOJI}/emoji-test.txt"

# The app ships English and Spanish. Order matters: it is the column order.
LANGUAGES = ["en", "es"]

# Skin-tone modifiers. Every human emoji has five tone variants in CLDR, which
# would triple the table and bury the plain forms under near-identical rows.
SKIN_TONES = {chr(c) for c in range(0x1F3FB, 0x1F400)}

VARIATION_SELECTOR_16 = "️"

# "1F600 ; fully-qualified # 😀 E1.0 grinning face"
TEST_LINE = re.compile(r"^([0-9A-F ]+?)\s*;\s*fully-qualified\s*#")


def fetch(url: str) -> str:
    print(f"  fetching {url}")
    with urllib.request.urlopen(url, timeout=60) as response:
        return response.read().decode("utf-8")


def rgi_emoji() -> list:
    """Every fully-qualified RGI emoji, in the order Unicode lists them."""
    emoji = []
    for line in fetch(EMOJI_TEST).splitlines():
        match = TEST_LINE.match(line)
        if not match:
            continue
        sequence = "".join(chr(int(cp, 16)) for cp in match.group(1).split())
        if not any(c in SKIN_TONES for c in sequence):
            emoji.append(sequence)
    return emoji


def annotations(language: str) -> dict:
    """Merge a language's annotations with its derived ones (flags, ZWJ
    sequences and other multi-codepoint emoji live in the derived file)."""
    entries = {}
    for kind in ("annotations", "annotationsDerived"):
        root = ET.fromstring(fetch(f"{CLDR_BASE}/{kind}/{language}.xml"))
        for node in root.iter("annotation"):
            key = node.get("cp")
            name, keywords = entries.get(key, ("", ""))
            if node.get("type") == "tts":
                name = (node.text or "").strip()
            else:
                # "face | grin | person" -> "face grin person"
                keywords = " ".join(part.strip() for part in (node.text or "").split("|"))
            entries[key] = (name, keywords)
    return entries


def look_up(entries: dict, emoji: str) -> tuple:
    """CLDR keys some sequences without the emoji-presentation selector that
    emoji-test.txt carries (❤️ vs ❤), so fall back to the stripped form."""
    if emoji in entries:
        return entries[emoji]
    return entries.get(emoji.replace(VARIATION_SELECTOR_16, ""), ("", ""))


def main() -> int:
    print(f"Unicode emoji {EMOJI}, CLDR {CLDR}")
    wanted = rgi_emoji()
    by_language = {language: annotations(language) for language in LANGUAGES}

    rows = []
    unnamed = []
    for emoji in wanted:
        english = look_up(by_language["en"], emoji)
        if not english[0]:
            # No English name means nothing to show and nothing to search.
            unnamed.append(emoji)
            continue
        columns = [emoji]
        for language in LANGUAGES:
            name, keywords = look_up(by_language[language], emoji)
            # Fall back to English so a gap in a translation still lists and
            # still matches, rather than showing an empty row.
            columns.append(name or english[0])
            columns.append(keywords or english[1])
        if any("\t" in column or "\n" in column for column in columns):
            print(f"  skipping {emoji!r}: a field contains a tab or newline")
            continue
        rows.append("\t".join(columns))

    if unnamed:
        print(f"  {len(unnamed)} emoji have no CLDR name and were left out: {''.join(unnamed)}")

    text = "\n".join(rows) + "\n"
    OUT_PATH.write_text(text, encoding="utf-8", newline="\n")
    size = len(text.encode("utf-8")) / 1024
    print(f"wrote {OUT_PATH.relative_to(ROOT)}: {len(rows)} emoji, {size:.0f} KiB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
