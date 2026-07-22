#!/usr/bin/env python3
"""Compile the Spanish .po into the shipped .mo, or verify the two agree.

The .mo is a committed binary artifact, so it silently goes stale whenever
someone edits the .po without recompiling — translations then fall back to
English at runtime with no error. Run from the repo root:

    python scripts/compile_catalog.py            # compile .po -> .mo
    python scripts/compile_catalog.py --check     # fail if the .mo is stale

Needs GNU gettext's msgfmt. On Windows: winget install mlocati.GetText
(note that a fresh terminal is needed afterwards for PATH to pick it up).
"""
import argparse
import gettext
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LC_MESSAGES = ROOT / "assets" / "locale" / "es" / "LC_MESSAGES"
PO_PATH = LC_MESSAGES / "launchtype.po"
MO_PATH = LC_MESSAGES / "launchtype.mo"

# msgfmt is often installed without landing on PATH; these are the spots it
# turns up on this project's dev machines (NVDA bundles a copy of its own).
FALLBACK_MSGFMT = [
    Path.home() / "AppData/Local/Programs/gettext-iconv/bin/msgfmt.exe",
    Path("C:/Program Files/gettext-iconv/bin/msgfmt.exe"),
    Path("C:/Program Files/NVDA/miscDeps/tools/msgfmt.exe"),
    Path("/opt/homebrew/bin/msgfmt"),
    Path("/usr/local/bin/msgfmt"),
]


def find_msgfmt() -> str:
    found = shutil.which("msgfmt")
    if found:
        return found
    for candidate in FALLBACK_MSGFMT:
        if candidate.is_file():
            return str(candidate)
    sys.exit(
        "msgfmt not found. Install GNU gettext:\n"
        "  Windows  winget install mlocati.GetText\n"
        "  macOS    brew install gettext\n"
        "  Linux    apt install gettext"
    )


def compile_to(msgfmt: str, destination: Path) -> None:
    result = subprocess.run(
        [msgfmt, "--check", "-o", str(destination), str(PO_PATH)],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        sys.exit(f"msgfmt failed:\n{result.stderr.strip()}")


def catalog(path: Path) -> dict:
    with path.open("rb") as handle:
        # The empty msgid holds the header metadata, which differs between
        # msgfmt versions; only the translations themselves matter here.
        return {k: v for k, v in gettext.GNUTranslations(handle)._catalog.items() if k}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify the committed .mo matches the .po instead of rewriting it",
    )
    args = parser.parse_args()
    msgfmt = find_msgfmt()

    if not args.check:
        compile_to(msgfmt, MO_PATH)
        print(f"{len(catalog(MO_PATH))} translations compiled -> {MO_PATH.relative_to(ROOT)}")
        return 0

    if not MO_PATH.is_file():
        print(f"{MO_PATH.relative_to(ROOT)} is missing; run without --check")
        return 1
    with tempfile.TemporaryDirectory() as tmp:
        fresh = Path(tmp) / "fresh.mo"
        compile_to(msgfmt, fresh)
        expected, actual = catalog(fresh), catalog(MO_PATH)

    if expected == actual:
        print(f"{len(actual)} translations; the shipped .mo matches the .po")
        return 0

    print("the shipped .mo is stale; run: python scripts/compile_catalog.py")
    for msgid in sorted(expected.keys() - actual.keys()):
        print(f"  missing  {msgid!r}")
    for msgid in sorted(actual.keys() - expected.keys()):
        print(f"  removed  {msgid!r}")
    for msgid in sorted(k for k in expected.keys() & actual.keys() if expected[k] != actual[k]):
        print(f"  changed  {msgid!r}: {actual[msgid]!r} -> {expected[msgid]!r}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
