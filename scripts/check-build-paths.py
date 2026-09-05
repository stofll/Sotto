#!/usr/bin/env python3
"""Проверить, что в раздаваемом артефакте нет путей сборочной машины.

    python scripts/check-build-paths.py desktop/src-tauri/target/release/Sotto.exe

Зачем. rustc вшивает `file!()` каждой зависимости в сообщения паники, а MSVC —
`__FILE__` в ассерты whisper.cpp. И то и другое — абсолютные пути той машины,
где собирали: домашний каталог с именем пользователя ОС и каталог сборки. В
раздаваемом бинаре им делать нечего. Подробности и замеры — в issue #41.

Скрипт ничего не чинит, он только ловит регрессию: флаги ремапа живут в
`scripts/build-installer.sh`, и первая же сборка мимо этого скрипта вернёт
пути обратно незамеченными.

Ищем в сыром байтовом содержимом, а не в извлечённых строках: пути лежат и в
UTF-8, и в UTF-16, и внутри сжатых секций — построчный разбор формата PE тут
дал бы меньше, чем простой поиск подстроки.

Выход: 0 — чисто, 1 — найдены следы, 2 — файл не читается.
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
    """Один и тот же каталог в тех видах, в каких он может лежать в бинаре.

    Пути приезжают из разных инструментов: rustc пишет их через прямой слэш,
    MSVC — через обратный, а в UTF-16-секциях каждый байт разделён нулём.
    """
    text = str(path)
    forms = {text, text.replace("\\", "/"), text.replace("/", "\\")}
    out: list[bytes] = []
    for form in forms:
        out.append(form.encode("utf-8"))
        out.append(form.encode("utf-16-le"))
    return out


def checks() -> list[tuple[str, list[bytes]]]:
    """Что ищем. Порядок — от «однозначно утечка» к «след машины»."""
    return [
        # The user's home directory: it carries the OS account name.
        ("домашний каталог пользователя", variants("C:\\Users\\")),
        # The crate registry is the same home directory, but it has its own
        # reason to end up in the binary (file!() of dependencies) and its own
        # remap, so it gets its own check.
        ("реестр cargo", variants(CARGO_HOME)),
        # The working copy: the maintainer's directory name and disk layout.
        ("рабочая копия", variants(REPO_ROOT)),
    ]


def scan(path: Path) -> int:
    try:
        data = path.read_bytes()
    except OSError as e:
        print(f"не прочитать {path}: {e}", file=sys.stderr)
        return 2

    print(f"{path} — {len(data) / 1024 / 1024:.1f} МБ")
    found = False
    for label, needles in checks():
        hits = sum(len(re.findall(re.escape(n), data)) for n in needles)
        mark = "FAIL" if hits else "ok"
        print(f"  [{mark:4}] {label}: {hits}")
        found = found or hits > 0

    if found:
        print(
            "\nВ артефакте остались пути сборочной машины. Собирайте релиз "
            "через scripts/build-installer.sh — он выставляет ремап; см. #41.",
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
