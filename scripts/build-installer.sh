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

# The public PostHog ingest token. It is baked into the binary at compile time
# through option_env! in telemetry.rs — that is, it is decided here, not at
# runtime. Without it telemetry is not "switched off" but absent: accepting() is
# always false, the delivery worker and the session watcher never start, and the
# linker throws the whole send path out of the binary. From the outside such a
# build is indistinguishable from a working one — the toggle in Settings still
# reads "on" while the dashboard stays silent.
#
# The token lives next to the signing key, outside the repository: it must not be
# committed. An environment variable that is already set takes priority, so that
# CI and a one-off build for somebody else's project need not touch the file.
posthog_key_file="${SOTTO_POSTHOG_KEY_PATH:-$HOME/.tauri/sotto-posthog.key}"
if [[ -z "${SOTTO_POSTHOG_API_KEY:-}" && -f "$posthog_key_file" ]]; then
    # tr rather than $(cat): a file created from PowerShell arrives with CRLF
    # and a trailing newline, and the token is compared against empty as is.
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

# Otherwise the build machine's paths travel into the distributed binary: rustc
# bakes the file!() of every dependency into panic messages, and that is
# $CARGO_HOME/registry — the home directory and the username of whoever built it.
#
# The stock profile `trim-paths` would do the same declaratively, but in Cargo
# 1.95 it is still unstable and the build fails on it, demanding nightly.
#
# A caveat: this is an rustc flag, it does not reach cmake. The paths to the
# whisper.cpp sources are baked in by MSVC through __FILE__; those are removed by
# the neutral build directory below, not by this flag.
#
# The separator is \x1f rather than a space: CARGO_ENCODED_RUSTFLAGS splits the
# string on it, so a path with a space in its name does not fall apart into two
# flags.
cargo_home_win=$(cygpath -w "${CARGO_HOME:-$USERPROFILE/.cargo}")
repo_root_win=$(cygpath -w "$(cd .. && pwd)")
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$cargo_home_win=/cargo"$'\x1f'"--remap-path-prefix=$repo_root_win=/build"

# The second half of the same task — the whisper.cpp paths. MSVC bakes those in
# through __FILE__ when building via cmake, and the rustc flag never reaches
# cl.exe: clang-cl has `-ffile-prefix-map`, while MSVC has only the undocumented
# `/d1trimfile:`. Rather than rely on an undocumented flag, the build directory
# itself is moved out of the working copy: the whisper.cpp sources are unpacked
# into OUT_DIR, that is under target/, and travel to a neutral place along with
# it. Only "$build_dir\release\build\..." ends up in the path — neither the
# username nor the name of the working-copy directory.
#
# The price is a separate build tree: the release does not reuse the objects of
# an ordinary `cargo build`, and the first build after this change compiles
# whisper.cpp from scratch. The directory is taken on the working copy's drive so
# as not to run out of space on the system one; override it with SOTTO_BUILD_DIR.
#
# The path is written with a forward slash: cargo and cmake both understand it,
# whereas a backslash inside `${x:-...}` would have to be escaped twice and would
# silently produce "D:sotto-build".
build_dir_default="${repo_root_win%%:*}:/sotto-build"
build_dir=${SOTTO_BUILD_DIR:-$build_dir_default}
export CARGO_TARGET_DIR="$build_dir"

# Not exec: the artefact has to be checked after the build, and exec would
# replace the process.
# The `.\` prefix is required: with NoDefaultCurrentDirectoryInExePath set,
# cmd refuses to run a script found only in the current directory.
cmd //c '.\build-installer.bat'

# The regression is caught here rather than by eye at the next release: any
# build that bypasses this script puts the paths back, and that cannot be
# spotted by looking at the binary.
exe="$(cygpath -u "$build_dir")/release/Sotto.exe"

python check-build-paths.py "$exe"

# Check the artefact, not the variable. The variable can be set and still fail
# to reach rustc — then the send path falls out of the binary along with the
# ingest address, and the only other way to notice is an empty dashboard a week
# later. The string is POSTHOG_CAPTURE_URL from telemetry.rs.
if [[ "${SOTTO_ALLOW_NO_TELEMETRY:-}" != "1" ]]; then
    if grep -aqF "eu.i.posthog.com" "$exe"; then
        echo "[build-installer] телеметрия: адрес ingest в бинаре есть"
    else
        echo "[build-installer] в бинаре нет адреса ingest PostHog:" >&2
        echo "[build-installer] телеметрия скомпилирована в no-op." >&2
        exit 1
    fi
fi
