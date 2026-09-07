#!/bin/sh
# `sherpa-onnx-sys` downloads its native archive over the network and extracts
# it without checking a hash. Fetch it here instead, verify it against
# scripts/sherpa-runtime.lock, and let the caller point SHERPA_ONNX_LIB_DIR at
# the result — the build script then downloads nothing. See AGENTS.md:
# downloaded assets stay verified.
#
# Usage: fetch-sherpa-runtime.sh [target-key] [cache-directory]
set -eu

target=${1:-osx-arm64-static}
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
lock_file="$script_dir/sherpa-runtime.lock"

version=$(awk '$1 == "version" { print $2 }' "$lock_file")
archive=$(awk -v key="$target" '$1 == key { print $2 }' "$lock_file")
expected=$(awk -v key="$target" '$1 == key { print $3 }' "$lock_file")
[ -n "$version" ] || { echo "No version in $lock_file" >&2; exit 1; }
[ -n "$archive" ] || { echo "No entry for target '$target' in $lock_file" >&2; exit 1; }

cache_dir=${2:-}
if [ -z "$cache_dir" ]; then
  cache_dir="${CARGO_TARGET_DIR:-$script_dir/../desktop/src-tauri/target}/sherpa-runtime"
fi
mkdir -p "$cache_dir"
cache_dir=$(CDPATH= cd -- "$cache_dir" && pwd)

archive_path="$cache_dir/$archive"
lib_dir="$cache_dir/${archive%.tar.bz2}/lib"

if [ ! -f "$archive_path" ]; then
  url="https://github.com/k2-fsa/sherpa-onnx/releases/download/v$version/$archive"
  echo "Downloading $url"
  # Into a temporary name first: an interrupted download must not be mistaken
  # for a cached archive on the next run.
  curl -fsSL --retry 3 -o "$archive_path.partial" "$url"
  mv "$archive_path.partial" "$archive_path"
fi

actual=$(shasum -a 256 "$archive_path" | cut -d' ' -f1)
if [ "$actual" != "$expected" ]; then
  # Remove it: leaving an archive that failed verification on disk invites a
  # later run to pick it up from the cache branch above.
  rm -f "$archive_path"
  echo "Checksum mismatch for $archive: expected $expected, got $actual" >&2
  exit 1
fi
echo "Verified $archive ($expected)"

if [ ! -d "$lib_dir" ]; then
  tar -xjf "$archive_path" -C "$cache_dir"
fi
[ -d "$lib_dir" ] || { echo "No lib directory in $archive" >&2; exit 1; }

echo "SHERPA_ONNX_LIB_DIR=$lib_dir"
if [ -n "${GITHUB_ENV:-}" ]; then
  echo "SHERPA_ONNX_LIB_DIR=$lib_dir" >> "$GITHUB_ENV"
fi
