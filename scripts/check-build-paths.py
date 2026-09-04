#!/usr/bin/env python3
"""Проверить, что в раздаваемом артефакте нет путей сборочной машины.

    python scripts/check-build-paths.py desktop/src-tauri/target/release/whisper-desktop.exe

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

# Весь вывод скрипта русский, а Python берёт кодировку stdout из локали
# процесса. На англоязычном Windows-раннере это cp1252, в которой кириллицы
# нет: скрипт падал с UnicodeEncodeError на первом же print — то есть до
# того, как выполнить хоть одну проверку, и релизная джоба краснела, ничего
# в артефакте не проверив. Локаль раннера не наше дело, поэтому кодировку
# самих потоков фиксируем здесь, а не в каждом месте вызова.
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

# Каталоги, которые не должны попасть в артефакт. Считаются на месте: путь к
# рабочей копии и к CARGO_HOME у каждой машины свой, и зашивать сюда «D:\\
# Project\\speech-to-text» значило бы проверять только одну машину.
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
        # Домашний каталог пользователя: здесь имя учётной записи ОС.
        ("домашний каталог пользователя", variants("C:\\Users\\")),
        # Реестр крейтов — тот же домашний каталог, но у него отдельная
        # причина попадать в бинарь (file!() зависимостей), и ремап для него
        # отдельный, поэтому и проверка отдельная.
        ("реестр cargo", variants(CARGO_HOME)),
        # Рабочая копия: имя каталога мейнтейнера и раскладка его диска.
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
