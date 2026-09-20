#!/bin/sh
# Sherpa and ONNX Runtime must be embedded, not resolved from a build cache.
set -eu
binary=${1:?usage: check-macos-native-libs.sh /path/to/Sotto}
dependencies=$(otool -L "$binary")
printf '%s\n' "$dependencies"
if printf '%s\n' "$dependencies" | tail -n +2 | grep -Eiv '^[[:space:]]*(/usr/lib/|/System/Library/)' ; then
  echo 'Unexpected non-system dynamic dependency in the macOS binary' >&2
  exit 1
fi
