#!/usr/bin/env python3
"""Check every tr("...") literal in the Rust sources against the .po catalog.

A msgid that is missing from the catalog will silently fall back to English
at runtime, so any drift from the Python-era msgids is a bug. Run from the
repo root:  python scripts/check_msgids.py
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PO_PATH = ROOT / "assets" / "locale" / "es" / "LC_MESSAGES" / "launchtype.po"

# The trailing comma is optional: rustfmt adds one whenever a long literal is
# broken onto its own line, and without it here those strings were silently
# skipped — exactly the ones most likely to be missing a translation.
TR_RE = re.compile(r'\btr\(\s*"((?:[^"\\]|\\.)*)"\s*,?\s*\)', re.DOTALL)


def unescape(rust_literal: str) -> str:
    # A backslash at end of line continues a Rust string literal: the newline
    # and the indentation that follows it are not part of the value. Undo that
    # first, or a wrapped msgid never matches its catalog entry.
    without_continuations = re.sub(r"\\\n\s*", "", rust_literal)
    return (
        without_continuations.replace('\\"', '"')
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\\\", "\\")
    )


def po_msgids(path: Path) -> set:
    msgids = set()
    current = None
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line.startswith("msgid "):
            current = eval(line[6:])  # noqa: S307 - po string literal
        elif line.startswith('"') and current is not None:
            current += eval(line)  # noqa: S307
        elif line.startswith("msgstr"):
            if current:
                msgids.add(current)
            current = None
    return msgids


def main() -> int:
    msgids = po_msgids(PO_PATH)
    missing = {}
    total = 0
    for rs in (ROOT / "crates").rglob("*.rs"):
        text = rs.read_text(encoding="utf-8")
        for match in TR_RE.finditer(text):
            literal = unescape(match.group(1))
            total += 1
            if literal not in msgids:
                missing.setdefault(literal, []).append(rs.relative_to(ROOT))
    print(f"{total} tr() literals checked against {len(msgids)} catalog msgids")
    if missing:
        print(f"\n{len(missing)} literals NOT in the Spanish catalog:")
        for literal, files in sorted(missing.items()):
            print(f"  {literal!r}  [{files[0]}]")
        return 1
    print("all tr() literals present in the catalog")
    return 0


if __name__ == "__main__":
    sys.exit(main())
