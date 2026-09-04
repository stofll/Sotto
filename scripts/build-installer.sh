#!/usr/bin/env bash
# Build the Windows NSIS installer for Sotto.
#
#   bash scripts/build-installer.sh
#
# Why a shell wrapper around a .bat, on Windows.
#
# The updater signing key is encrypted with an empty password. Tauri takes it
# from TAURI_SIGNING_PRIVATE_KEY_PASSWORD and prompts interactively when the
# variable is absent — which hangs an unattended build. cmd cannot help here:
# `set VAR=` deletes a variable rather than emptying it, so the .bat cannot
# create the value it needs. A POSIX shell can, and the empty string does reach
# the build through cmd — only cmd's own `if defined` misreports it as absent.
# Hence the marker variable below: it is what the .bat is able to check.
set -euo pipefail

cd "$(dirname "$0")"

# Empty on purpose. This is the password.
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=
export SOTTO_SIGNING_PASSWORD_EXPORTED=1

# Публичный ingest-токен PostHog. Он вшивается в бинарь на этапе компиляции
# через option_env! в telemetry.rs — то есть решается здесь, а не в рантайме.
# Без него телеметрия не «выключена», а отсутствует: accepting() всегда false,
# воркер доставки и session-watcher не стартуют, и линкер выбрасывает весь
# путь отправки из бинаря. Снаружи такая сборка неотличима от рабочей —
# тумблер в Settings всё равно показывает «включено», а дашборд молчит.
#
# Токен лежит рядом с ключом подписи, вне репозитория: в коммит ему нельзя.
# Уже выставленная переменная окружения имеет приоритет, чтобы CI и разовая
# сборка на чужой проект не требовали трогать файл.
posthog_key_file="${SOTTO_POSTHOG_KEY_PATH:-$HOME/.tauri/sotto-posthog.key}"
if [[ -z "${SOTTO_POSTHOG_API_KEY:-}" && -f "$posthog_key_file" ]]; then
    # tr, а не $(cat): в файле, созданном из PowerShell, приезжает CRLF и
    # завершающий перевод строки, а токен сравнивается с пустым как есть.
    SOTTO_POSTHOG_API_KEY=$(tr -d '[:space:]' < "$posthog_key_file")
fi
export SOTTO_POSTHOG_API_KEY

if [[ -z "${SOTTO_POSTHOG_API_KEY:-}" ]]; then
    if [[ "${SOTTO_ALLOW_NO_TELEMETRY:-}" != "1" ]]; then
        echo "[build-installer] нет ingest-токена PostHog." >&2
        echo "[build-installer] Положите публичный токен проекта (phc_...) в:" >&2
        echo "[build-installer]   $posthog_key_file" >&2
        echo "[build-installer] Сборка без телеметрии — SOTTO_ALLOW_NO_TELEMETRY=1." >&2
        exit 1
    fi
    echo "[build-installer] SOTTO_ALLOW_NO_TELEMETRY=1 — собираем без телеметрии"
fi

# Пути сборочной машины иначе уезжают в раздаваемый бинарь: rustc вшивает
# file!() каждой зависимости в сообщения паники, а это $CARGO_HOME/registry —
# то есть домашний каталог и имя пользователя того, кто собирал.
#
# Штатный профильный `trim-paths` сделал бы то же самое декларативно, но в
# Cargo 1.95 он всё ещё нестабилен и сборка на нём падает, требуя nightly.
#
# Оговорка: это флаг rustc, до cmake он не достаёт. Пути к исходникам
# whisper.cpp вшивает MSVC через __FILE__; их убирает нейтральный каталог
# сборки ниже, а не этот флаг.
#
# Разделитель — \x1f, а не пробел: CARGO_ENCODED_RUSTFLAGS режет строку
# по нему, и путь с пробелом в имени не разъедется на два флага.
cargo_home_win=$(cygpath -w "${CARGO_HOME:-$USERPROFILE/.cargo}")
repo_root_win=$(cygpath -w "$(cd .. && pwd)")
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$cargo_home_win=/cargo"$'\x1f'"--remap-path-prefix=$repo_root_win=/build"

# Вторая половина той же задачи — пути whisper.cpp. Их вшивает MSVC через
# __FILE__ при сборке через cmake, и флаг rustc до cl.exe не доходит:
# `-ffile-prefix-map` есть у clang-cl, а у MSVC — только недокументированный
# `/d1trimfile:`. Вместо того чтобы полагаться на недокументированный флаг,
# уводим сам каталог сборки из рабочей копии: исходники whisper.cpp
# распаковываются в OUT_DIR, то есть под target/, и вместе с ним переезжают
# в нейтральное место. В путь попадает только «$build_dir\release\build\…» —
# ни имени пользователя, ни имени каталога рабочей копии.
#
# Цена — отдельное дерево сборки: релиз не переиспользует объекты обычного
# `cargo build`, и первая сборка после этой правки собирает whisper.cpp с
# нуля. Каталог берётся на диске рабочей копии, чтобы не упереться в место
# на системном; переопределяется через SOTTO_BUILD_DIR.
#
# Путь пишется через прямой слэш: cargo и cmake его понимают, а обратный в
# `${x:-...}` пришлось бы экранировать дважды и молча получить «D:sotto-build».
build_dir_default="${repo_root_win%%:*}:/sotto-build"
build_dir=${SOTTO_BUILD_DIR:-$build_dir_default}
export CARGO_TARGET_DIR="$build_dir"

# Не exec: после сборки надо проверить артефакт, а exec заменил бы процесс.
# The `.\` prefix is required: with NoDefaultCurrentDirectoryInExePath set,
# cmd refuses to run a script found only in the current directory.
cmd //c '.\build-installer.bat'

# Регрессию ловим здесь, а не глазами при следующем релизе: любая сборка
# мимо этого скрипта вернёт пути обратно, и заметить это по бинарю нельзя.
exe="$(cygpath -u "$build_dir")/release/whisper-desktop.exe"

python check-build-paths.py "$exe"

# Проверяем артефакт, а не переменную. Переменная может быть выставлена и всё
# равно не доехать до rustc — тогда отправка выпадает из бинаря вместе с
# адресом ingest, и единственный способ это заметить иначе — пустой дашборд
# через неделю. Строка — POSTHOG_CAPTURE_URL из telemetry.rs.
if [[ "${SOTTO_ALLOW_NO_TELEMETRY:-}" != "1" ]]; then
    if grep -aqF "eu.i.posthog.com" "$exe"; then
        echo "[build-installer] телеметрия: адрес ingest в бинаре есть"
    else
        echo "[build-installer] в бинаре нет адреса ingest PostHog:" >&2
        echo "[build-installer] телеметрия скомпилирована в no-op." >&2
        exit 1
    fi
fi
