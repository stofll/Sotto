#!/bin/sh
# check-version.sh — verify that every shipped version source agrees.
#
# Usage:
#   sh scripts/check-version.sh [vX.Y.Z]
#
# The optional tag is checked against the application version.  Keeping this
# check independent of git state lets CI run it on a detached tag checkout and
# lets scripts/release.sh use the same source-of-truth checks locally.

set -eu

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$REPO_ROOT"

fail() {
    printf '[FAIL] %s\n' "$*" >&2
    exit 1
}

read_version() {
    file=$1
    label=$2
    value=$3
    [ -n "$value" ] || fail "Could not read $label from $file"
    printf '%s\n' "$value"
}

# Keep the accepted format aligned with docs/RELEASE.md → Tag Format: normal
# SemVer plus optional pre-release/build metadata (for example 0.3.0-rc.1).
is_semver() {
    printf '%s\n' "$1" | grep -Eq \
        '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'
}

CARGO_TOML='desktop/src-tauri/Cargo.toml'
CARGO_LOCK='desktop/src-tauri/Cargo.lock'
PACKAGE_JSON='desktop/package.json'
INFO_PLIST='desktop/src-tauri/Info.plist'
README='README.md'

cargo_version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$CARGO_TOML" | head -n 1)
cargo_lock_version=$(awk '
    /^\[\[package\]\]$/ { in_package = 1; is_app = 0; next }
    in_package && /^name = "sotto"$/ { is_app = 1; next }
    is_app && /^version = "/ {
        line = $0
        sub(/^version = "/, "", line)
        sub(/".*$/, "", line)
        print line
        exit
    }
' "$CARGO_LOCK")
package_version=$(sed -n 's/^[[:space:]]*"version":[[:space:]]*"\([^"]*\)".*/\1/p' "$PACKAGE_JSON" | head -n 1)
plist_version=$(awk '
    /<key>CFBundleShortVersionString<\/key>/ {
        if (getline > 0) {
            line = $0
            sub(/^.*<string>/, "", line)
            sub(/<\/string>.*$/, "", line)
            print line
        }
    }
' "$INFO_PLIST")
readme_version=$(awk '
    /^## Current Status$/ { in_status = 1; next }
    in_status && /^## / { in_status = 0 }
    in_status && /^- Version:[[:space:]]*`[^`]+`/ {
        line = $0
        sub(/^[^0-9]*/, "", line)
        sub(/[^0-9A-Za-z.-].*$/, "", line)
        print line
        exit
    }
' "$README")

cargo_version=$(read_version "$CARGO_TOML" 'package version' "$cargo_version")
cargo_lock_version=$(read_version "$CARGO_LOCK" 'locked package version' "$cargo_lock_version")
package_version=$(read_version "$PACKAGE_JSON" 'package version' "$package_version")
plist_version=$(read_version "$INFO_PLIST" 'CFBundleShortVersionString' "$plist_version")
readme_version=$(read_version "$README" 'Current Status version' "$readme_version")

is_semver "$cargo_version" || fail "Version '$cargo_version' is not valid SemVer"

for pair in \
    "Cargo.lock:$cargo_lock_version" \
    "package.json:$package_version" \
    "Info.plist:$plist_version" \
    "README.md:$readme_version"; do
    name=${pair%%:*}
    value=${pair#*:}
    [ "$value" = "$cargo_version" ] || fail \
        "Version mismatch: Cargo.toml=$cargo_version, $name=$value"
done

if [ "$#" -gt 1 ]; then
    fail "Usage: sh scripts/check-version.sh [vX.Y.Z]"
fi
if [ "$#" -eq 1 ]; then
    expected_tag=$1
    case "$expected_tag" in
        v*) expected_version=${expected_tag#v} ;;
        *) fail "Release tag '$expected_tag' must start with v" ;;
    esac
    [ "$expected_version" = "$cargo_version" ] || fail \
        "Tag '$expected_tag' does not match application version '$cargo_version'"
fi

printf '[PASS] application version: %s\n' "$cargo_version"
printf '[PASS] Cargo.toml, Cargo.lock, package.json, Info.plist and README.md agree\n'
if [ "$#" -eq 1 ]; then
    printf '[PASS] release tag: %s\n' "$1"
fi
