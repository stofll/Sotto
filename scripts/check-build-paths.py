#!/usr/bin/env python3
"""Check that a distributed artifact carries no build machine paths.

    python scripts/check-build-paths.py desktop/src-tauri/target/release/Sotto.exe

Why. rustc bakes every dependency's `file!()` into panic messages, and MSVC
bakes `__FILE__` into the whisper.cpp asserts. Both are absolute paths of the
machine that built it: the home directory carrying the OS account name, and the
build directory. Neither belongs in a distributed binary. Details and
measurements are in issue #41.

This script fixes nothing, it only catches a regression: the remap flags live in
`scripts/build-installer.sh`, and the first build that bypasses this script
would bring the paths back unnoticed.

The search runs over the raw bytes rather than extracted strings: the paths sit
in UTF-8, in UTF-16 and inside compressed sections alike, and parsing the PE
format properly would buy less here than a plain substring search.

Exit codes: 0 clean, 1 traces found, 2 file unreadable.
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

# All of this script's output is Russian, and Python takes the stdout encoding
# from the process locale. On an English-language Windows runner that is cp1252,
# which has no Cyrillic: the script died with UnicodeEncodeError on the very
# first print — that is, before performing a single check, and the release job
# went red without having inspected anything in the artifact. The runner's locale
# is not our business, so we pin the encoding of the streams themselves here
# rather than at every call site.
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

# Directories that must not end up in the artifact. Computed on the spot: the
# path to the working copy and to CARGO_HOME differs on every machine, and
# hardcoding «D:\\Project\\speech-to-text» here would mean checking one machine
# only.
REPO_ROOT = Path(__file__).resolve().parent.parent
CARGO_HOME = Path(os.environ.get("CARGO_HOME") or Path.home() / ".cargo")


def variants(path: Path | str) -> list[bytes]:
    """The same directory in every form it may take inside the binary.

    The paths arrive from different tools: rustc writes them with a forward
    slash, MSVC with a backslash, and in UTF-16 sections every byte is
    separated by a zero.
    """
    text = str(path)
    forms = {text, text.replace("\\", "/"), text.replace("/", "\\")}
    out: list[bytes] = []
    for form in forms:
        out.append(form.encode("utf-8"))
        out.append(form.encode("utf-16-le"))
    return out


def checks() -> list[tuple[str, list[bytes]]]:
    """What to look for, ordered from "definitely a leak" to "a trace of the machine"."""
    return [
        # The user's home directory: it carries the OS account name.
        ("user home directory", variants("C:\\Users\\")),
        # The crate registry is the same home directory, but it has its own
        # reason to end up in the binary (file!() of dependencies) and its own
        # remap, so it gets its own check.
        ("cargo registry", variants(CARGO_HOME)),
        # The working copy: the maintainer's directory name and disk layout.
        ("working copy", variants(REPO_ROOT)),
    ]


def scan(path: Path) -> int:
    try:
        data = path.read_bytes()
    except OSError as e:
        print(f"cannot read {path}: {e}", file=sys.stderr)
        return 2

    print(f"{path} — {len(data) / 1024 / 1024:.1f} MB")
    found = False
    for label, needles in checks():
        hits = sum(len(re.findall(re.escape(n), data)) for n in needles)
        mark = "FAIL" if hits else "ok"
        print(f"  [{mark:4}] {label}: {hits}")
        found = found or hits > 0

    if found:
        print(
            "\nBuild machine paths are still in the artifact. Build the release "
            "through scripts/build-installer.sh, which sets the remap; see #41.",
            file=sys.stderr,
        )
    return 1 if found else 0


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    return scan(Path(argv[1]))


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
