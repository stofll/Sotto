#!/bin/sh
# Build the macOS disk image from a bundled Sotto.app with the layout in
# scripts/macos-dmg and the artwork in desktop/src-tauri/installer/macos.
# Requires uv. Signs the image when APPLE_SIGNING_IDENTITY is set, the same
# identity the bundler signs the application with.
#
#   sh scripts/build-dmg.sh <Sotto.app> <output.dmg>
set -eu

if [ $# -ne 2 ]; then
  echo "usage: sh scripts/build-dmg.sh <Sotto.app> <output.dmg>" >&2
  exit 2
fi
if [ ! -d "$1/Contents/MacOS" ]; then
  echo "not an application bundle: $1" >&2
  exit 1
fi
if [ -e "$2" ]; then
  echo "refusing to overwrite $2" >&2
  exit 1
fi

root=$(cd "$(dirname "$0")/.." && pwd)
artwork="$root/desktop/src-tauri/installer/macos"
app=$(cd "$1" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# A multi-resolution TIFF lets Finder pick the @2x artwork on Retina displays.
tiffutil -cathidpicheck "$artwork/background.png" "$artwork/background@2x.png" \
  -out "$work/background.tiff" > /dev/null

uv run --locked --project "$root/scripts/macos-dmg" dmgbuild \
  -s "$root/scripts/macos-dmg/dmg_settings.py" \
  -D app="$app" \
  -D background="$work/background.tiff" \
  -D volume_icon="$root/desktop/src-tauri/icons/icon.icns" \
  -D license="$root/LICENSE" \
  Sotto "$2"

if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  codesign --force --sign "$APPLE_SIGNING_IDENTITY" "$2"
fi
echo "Disk image: $2"
