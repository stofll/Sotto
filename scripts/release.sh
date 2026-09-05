#!/bin/sh
# release.sh — Dry-run release verification for Sotto
#
# Verifies:
#   1. git working tree is clean
#   2. current branch is main
#   3. all application version sources agree and are parseable
#   4. tag v$VERSION doesn't already exist
#
# Prints a colour-coded summary of what WOULD be done.
# No actual tagging, building, or pushing occurs.
#
# Usage: sh scripts/release.sh

set -eu

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$REPO_ROOT"

# ---------------------------------------------------------------------------
# Colour helpers (portable across macOS and Linux)
# ---------------------------------------------------------------------------
if [ -t 1 ]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[1;33m'
    CYAN='\033[0;36m'
    BOLD='\033[1m'
    NC='\033[0m' # No Colour
else
    RED=''
    GREEN=''
    YELLOW=''
    CYAN=''
    BOLD=''
    NC=''
fi

ok()   { printf "${GREEN}[PASS]${NC} %s\n" "$*"; }
warn() { printf "${YELLOW}[WARN]${NC} %s\n" "$*"; }
fail() { printf "${RED}[FAIL]${NC} %s\n" "$*"; }
info() { printf "${CYAN}[INFO]${NC} %s\n" "$*"; }

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------
CARGO_TOML="desktop/src-tauri/Cargo.toml"

echo "${BOLD}=== Sotto release dry-run ===${NC}"
echo ""

# ---------------------------------------------------------------------------
# 0. Version consistency
# ---------------------------------------------------------------------------
info "Checking version consistency..."
sh scripts/check-version.sh
ok "Cargo.toml, Cargo.lock, package.json, Info.plist and README.md agree."

# ---------------------------------------------------------------------------
# 1. Clean working tree
# ---------------------------------------------------------------------------
info "Checking working tree..."
working_tree_status=$(git status --porcelain --untracked-files=normal)
if [ -n "$working_tree_status" ]; then
    fail "Working tree is not clean (including untracked files)."
    printf '%s\n' "$working_tree_status"
    exit 1
fi
ok "Working tree is clean."

# ---------------------------------------------------------------------------
# 2. Branch check
# ---------------------------------------------------------------------------
info "Checking current branch..."
BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
if [ "$BRANCH" != "main" ]; then
    fail "Current branch is '$BRANCH', expected 'main'."
    exit 1
fi
ok "On branch '$BRANCH'."

# ---------------------------------------------------------------------------
# 3. Read version from Cargo.toml
# ---------------------------------------------------------------------------
info "Reading version from $CARGO_TOML..."
if [ ! -f "$CARGO_TOML" ]; then
    fail "File not found: $CARGO_TOML"
    exit 1
fi

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$CARGO_TOML" 2>/dev/null | head -1)
if [ -z "$VERSION" ]; then
    fail "Could not extract version from $CARGO_TOML"
    exit 1
fi

# Validate semver-ish format (basic check)
case "$VERSION" in
    [0-9]*.[0-9]*.[0-9]*)
        ok "Version: $VERSION"
        ;;
    *)
        fail "Version '$VERSION' does not look like semver (X.Y.Z)."
        exit 1
        ;;
esac

# ---------------------------------------------------------------------------
# 4. Tag existence check
# ---------------------------------------------------------------------------
TAG="v${VERSION}"
info "Checking tag '$TAG'..."
if git rev-parse --verify "refs/tags/$TAG" >/dev/null 2>&1; then
    fail "Tag '$TAG' already exists. Bump the version first."
    exit 1
fi
ok "Tag '$TAG' is available."

# ---------------------------------------------------------------------------
# 5. Dry-run summary
# ---------------------------------------------------------------------------
echo ""
echo "${BOLD}=== Dry-run summary (no changes made) ===${NC}"
echo ""
info "Releasing ${VERSION} would go like this."
echo ""

info "You run:"
printf "  ${CYAN}1.${NC} git tag -a -m 'release ${VERSION}' %s\n" "$TAG"
printf "  ${CYAN}2.${NC} git push origin %s\n" "$TAG"
echo ""

info ".github/workflows/release.yml then runs, on the tag push:"
printf "  ${CYAN}3.${NC} build+sign aarch64-apple-darwin (.dmg, .app.tar.gz)\n"
printf "  ${CYAN}4.${NC} build+sign x86_64-pc-windows-msvc (NSIS .exe)\n"
printf "  ${CYAN}5.${NC} attach latest.json and open a DRAFT release\n"
printf "  ${CYAN}6.${NC} attach the SBOM and the license report\n"
printf "  ${CYAN}7.${NC} attach SHA256SUMS.txt over the draft's own assets\n"
echo ""

info "You finish:"
printf "  ${CYAN}8.${NC} check the draft, write the release body, publish it\n"
printf "     Publishing is when latest.json goes live for the updater.\n"
echo ""

warn "Code signing (Windows Authenticode, Apple notarization) requires"
warn "secrets to be configured in CI. See docs/RELEASE.md for the checklist."
echo ""

echo "${BOLD}=== All checks passed. Ready to release v${VERSION}. ===${NC}"
