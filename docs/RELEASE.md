# Release Process

> **Maintainer runbook.** Step-by-step procedure for cutting a release,
> including the parts only the maintainer can perform (signing secrets,
> updater keys, rollback). Contributors want [`releasing.md`](releasing.md)
> instead. Where this file and `.github/workflows/release.yml` disagree, the
> workflow is authoritative — it is what actually runs.

> Applies to [Sotto](https://github.com/stofll/Sotto) — the
> native Rust/Tauri speech-to-text app (**Sotto**).

## Versioning Policy

This project follows **Semantic Versioning** (`MAJOR.MINOR.PATCH`).

| Bump | When | Example |
|------|------|---------|
| **MAJOR** | Tauri engine upgrade (1.x -> 2.x, 2.x -> 3.x), breaking IPC contract changes, migration to a new STT engine (e.g. replacing whisper-rs), dropped platform support | `0.1.0` -> `1.0.0` |
| **MINOR** | New Tauri command added to the public IPC surface, new LLM or cloud STT provider, new pipeline mode, new config section with migration, public API additions in `lib.rs` | `0.1.0` -> `0.2.0` |
| **PATCH** | Bug fixes, dependency updates, internal refactors, performance improvements, documentation changes that don't add new IPC commands or config sections | `0.1.0` -> `0.1.1` |

Pre-release versions follow `X.Y.Z-rc.N` during release-candidate staging.

---

## Pre-release

### 1. Version Bump

- Update `version` in `desktop/src-tauri/Cargo.toml`.
- Update `version` in `desktop/package.json`.
- Verify consistency: both files must carry the same version.
- Commit: `chore(release): bump version to X.Y.Z`.

### 2. Dependency Audit

```bash
# Check for outdated Rust dependencies
cd desktop/src-tauri
cargo outdated --exit-code 1

# Security audit (requires cargo-audit installed)
cargo audit

# Check for outdated npm packages
cd desktop
pnpm outdated
```

If advisories are found, upgrade affected dependencies in a separate PR before
proceeding with the release.

### 3. Lint and Format

```bash
cd desktop/src-tauri

# Format check
cargo fmt --all -- --check

# Clippy (deny warnings)
cargo clippy --all-targets -- -D warnings
```

### 4. Full Test Suite

```bash
# Rust tests (all targets)
cd desktop/src-tauri
cargo test --all-targets

# Frontend typecheck and tests
cd desktop
pnpm exec tsc --noEmit
pnpm test

# Frontend production build (catches Vite/ESBuild issues)
pnpm build

# Full Tauri build (smoke test)
pnpm tauri build
```

### 5. Code Signing Checklist

Two different signatures, often confused:

| | Proves | Where it lives | Set up? |
|---|---|---|---|
| **minisign** (updater) | the update artifact was not tampered with and came from the key holder | `TAURI_SIGNING_PRIVATE_KEY` secret; public half in `tauri.conf.json` | yes |
| **Authenticode** (Windows) | the *publisher* is who they claim to be — this is what silences SmartScreen | `WINDOWS_CERT_BASE64` secret | **no** |

Without Authenticode, both the first install and every update show
"unknown publisher". The updater works; it just looks untrustworthy. A
certificate has not been obtained, so every release so far ships unsigned in
the publisher sense.

- [ ] **Windows Authenticode certificate** loaded in CI secrets
      (`WINDOWS_CERT_BASE64`, `WINDOWS_CERT_PASSWORD`).
- [ ] **Apple Developer Program** certificate + notarization credentials in CI
      secrets (`APPLE_CERT_BASE64`, `APPLE_CERT_PASSWORD`,
      `APPLE_NOTARIZATION_USERNAME`, `APPLE_NOTarIZATION_PASSWORD`).
- [ ] `tauri.conf.json` has `bundle.windows.signing` and
      `bundle.macOS.signing` configured (or CI override).
- [ ] Test signing locally before tagging:
      `pnpm tauri build --bundles nsis` (Windows) /
      `pnpm tauri build --bundles dmg` (macOS).

### 6. Bundle Target List

These are the two targets the release workflow builds, and the bundle
formats `tauri.conf.json` declares (`"targets": ["dmg", "nsis"]`). There is no
MSI and no Intel-Mac build; see [Platform support](platforms.md) for what is
promised on each target.

| Platform | Bundle format | Target triple |
|----------|---------------|---------------|
| macOS arm64 | `.dmg` | `aarch64-apple-darwin` |
| Windows x64 | `.exe` (NSIS) | `x86_64-pc-windows-msvc` |

---

## Windows Installer

Everything the NSIS installer needs beyond `bundle.windows.nsis` in
`tauri.conf.json` lives in `desktop/src-tauri/installer/`:

| File | What it is |
|------|------------|
| `header.bmp` | 150×57, drawn in the wizard header band |
| `sidebar.bmp` | 164×314, the left strip of the welcome and finish pages |
| `installer.nsi` | fork of the bundler's own template |
| `hooks.nsh` | `NSIS_HOOK_*` macros |

**Artwork.** Both bitmaps must be 24-bit BMP at exactly those sizes — MUI2 does
not scale them, and a 32-bit BMP (GDI+'s default, alpha channel included)
renders as garbage. Regenerate them from the app icon with:

```bash
powershell -ExecutionPolicy Bypass -File scripts/make-installer-art.ps1
```

**Template fork.** `installer.nsi` is a copy of tauri-bundler's template with
three marked changes: branded welcome/finish copy as `LangString`s, `NOSTRETCH`
on both bitmaps, and the hooks `!include` moved below the `!define` block (in
upstream it sits above, so a hook referencing `${PRODUCTNAME}` silently gets an
empty string). Every edit is tagged `Sotto:` so a rebase onto a new upstream
template is a diff, not an archaeology exercise.

Pin check: the bundler version is not the CLI version. Read it from the CLI's
lockfile, then unpack that bundler to diff its template against ours:

```bash
tar xzf ~/.cargo/registry/cache/*/tauri-cli-<ver>.crate tauri-cli-<ver>/Cargo.lock
```

**Rename migration.** `hooks.nsh` removes an installation made under a previous
product name. The uninstall registry key is `Uninstall\${PRODUCTNAME}`, keyed on
the display name rather than the bundle id, so after the `Шёпот` → `Sotto`
rename the built-in "already installed" check no longer sees the old copy and
would leave two of everything. The hook finds it by `Publisher`, which the
bundler fills with the second segment of the identifier, and runs its
uninstaller in passive mode, which keeps user data.

Because the identifier moved too (`com.shepot.app` → `com.sotto.app`), that
`Publisher` no longer matches the current build: the installed copy is stamped
`shepot`, this one is `sotto`. The hook therefore carries `LEGACYPUBLISHER` and
matches either, reading the install directory out of `Software\<publisher>\…`
under whichever one hit. Drop the constant when the whole file goes.

It runs on the updater path too (`/UPDATE`), where it matters most: an update
from `Шёпот` installs into a new directory, and nothing else would ever clean
up the old one.

---

## Updater Keys (one-time setup)

The keypair was generated with:

```bash
pnpm exec tauri signer generate -w ~/.tauri/sotto.key
```

- **Public half** — already committed as `plugins.updater.pubkey` in
  `desktop/src-tauri/tauri.conf.json`. An installed build refuses any update
  whose signature does not match it.
- **Private half** — `~/.tauri/sotto.key`, never committed. Its contents go
  into the repository secret `TAURI_SIGNING_PRIVATE_KEY`, and the password
  (empty for this key) into `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

Losing the private key means shipped installations can no longer be updated:
they will reject artifacts signed by any replacement key, and every user has
to reinstall by hand. Back it up somewhere other than this machine.

---

## Tag & Build

Tagging is enough — `.github/workflows/release.yml` builds Windows and macOS
arm64, signs the artifacts, generates `latest.json` and attaches everything to
a **draft** release. Publishing that draft is what makes the update visible to
users, so check the build before you press it.

The release body is what the app shows as "what's new", so write it for users
rather than as a changelog dump.

### Tag Format

Tags follow `vX.Y.Z` (e.g., `v0.2.0`). Pre-release tags use
`vX.Y.Z-rc.N` (e.g., `v0.2.0-rc.1`).

```bash
# After version bump commit is on main
git tag v0.2.0
git push origin v0.2.0
```

### Telemetry Ingest Token

Release builds compile the public PostHog ingest token in at build time, so a
build either has telemetry or does not — the runtime toggle cannot add it back.
Built without the token, telemetry is a complete no-op: nothing is queued, no
worker runs, and the linker drops the delivery path out of the binary. Settings
still shows the toggle as on, so such a build is indistinguishable from a
working one until the dashboard stays empty.

The Windows build is the release path, and it refuses to produce a silent
no-op. `build-installer.sh` reads the token from `~/.tauri/sotto-posthog.key`
(next to the updater signing key, outside the repository); override the
location with `SOTTO_POSTHOG_KEY_PATH`, or set `SOTTO_POSTHOG_API_KEY` in the
environment to win over the file. Missing token — the build stops before
`cargo`. After the build, the script greps the artifact for the ingest host: a
token that never reached `rustc` fails there, which a check on the variable
alone would miss.

The release workflow takes the same variable from a repository secret of that
name. That secret is not set, so a CI build would carry no telemetry — release
builds are made on Windows through the script above.

Pass `SOTTO_ALLOW_NO_TELEMETRY=1` to build deliberately without telemetry; it
skips both the pre-build guard and the artifact check.

Create the local key file once:

```powershell
Set-Content -Path $env:USERPROFILE\.tauri\sotto-posthog.key -Value "phc_..." -Encoding ascii
```

The token is the public project ingest key, never a personal or
administrative PostHog key. See [telemetry.md](telemetry.md).

### Build Paths

A release binary must not carry the build machine's directory layout. Two
different tools bake it in, and each needs its own countermeasure:

| Source | What leaks | Fix |
|---|---|---|
| rustc `file!()` in dependency panic messages | `$CARGO_HOME/registry` — the **OS user name** | `--remap-path-prefix` (`CARGO_ENCODED_RUSTFLAGS` in `build-installer.sh`, `RUSTFLAGS` in `release.yml`) |
| MSVC `__FILE__` in whisper.cpp asserts | the build directory | build outside the working copy (`CARGO_TARGET_DIR`) |

`--remap-path-prefix` is an rustc flag and never reaches `cl.exe`; MSVC has no
`-ffile-prefix-map`, only the undocumented `/d1trimfile:`. So instead of a flag,
the release build moves the target directory itself: whisper.cpp is unpacked
into `OUT_DIR` and travels with it. `scripts/build-installer.sh` sets
`CARGO_TARGET_DIR` to `<repo drive>:/sotto-build`; override with
`SOTTO_BUILD_DIR`. The bundle therefore lands in
`$CARGO_TARGET_DIR/release/bundle/nsis`, **not** under `desktop/src-tauri`.

The cost is a second build tree: a release build shares nothing with a plain
`cargo build`, and the first one after this change compiles whisper.cpp from
scratch.

The profile-level `trim-paths` would replace the remap declaratively, but it
still requires nightly in Cargo 1.95. Swap it in when it stabilises.

Verify any binary you are about to hand out:

```bash
python scripts/check-build-paths.py <path-to-exe>
```

It runs automatically at the end of `build-installer.sh` and as a step in
`release.yml`. In CI the step runs *after* the artifacts are uploaded — the
release is a draft, so a red check is a reason not to publish it.

### Build Commands

Windows releases go through the wrapper, not `pnpm tauri build` directly — it
is what exports the signing password and the path flags above:

```bash
bash scripts/build-installer.sh
```

```bash
# macOS arm64
cd desktop
pnpm tauri build --bundles dmg --target aarch64-apple-darwin

# Windows x64
cd desktop
pnpm tauri build --bundles nsis --target x86_64-pc-windows-msvc
```

### Checksum Generation

```bash
# See "Build Paths": on Windows this is $CARGO_TARGET_DIR, not the working copy.
cd "${CARGO_TARGET_DIR:-desktop/src-tauri/target}/release/bundle"
sha256sum *.dmg *.exe 2>/dev/null > SHA256SUMS.txt
```

### Signing (placeholder)

```bash
# Windows: signtool sign
# signtool sign /fd SHA256 /a /f cert.pfx /p "$CERT_PASSWORD" *-setup.exe

# macOS: codesign + notarize
# codesign --force --options runtime --sign "$DEV_ID" --timestamp *.app
# ditto -c -k --keepParent *.app unsigned.zip
# xcrun notarytool submit unsigned.zip --apple-id ... --password ... --team-id ...
# xcrun stapler staple *.dmg
```

---

## Publish

### GitHub Release

1. Create a new Release on GitHub from the `vX.Y.Z` tag.
2. Use the auto-generated release-drafter notes as a starting point.
3. Fill in the "What's new" template below.

### What's New Template

```markdown
## Security

- [bullet point security fixes or "None in this release"]

## Performance

- [bullet point perf improvements or "None in this release"]

## New Features

- [bullet point new features or "None in this release"]

## Bug Fixes

- [bullet point bug fixes or "None in this release"]
```

### Asset Upload Order

1. `.dmg` (macOS arm64)
2. `.exe` (Windows NSIS installer)
3. `SHA256SUMS.txt`
4. Source code archive (auto-generated by GitHub)

---

## Post-release

### Monitoring

- Check crash-reporting dashboard (once configured) for new crash clusters
  within 24 hours.
- Monitor GitHub Issues for installation or runtime reports.
- Verify download counts and asset availability on the Release page.

### Rollback Procedure

The endpoint is `releases/latest/download/latest.json`, so "latest" is
whichever release GitHub currently marks as latest — that is the lever.

1. Mark the bad release as a pre-release (or delete it). GitHub then points
   "latest" at the previous release, and its `latest.json` takes over.
2. Users who have not updated yet see nothing at all.
3. Users who already updated are **not** downgraded automatically: the plugin
   compares versions and treats an older manifest as "no update available".
   They need the previous installer by hand, or a `vX.Y.Z+1` that reverts the
   change — the second option is usually the honest one.

Never re-tag a version that has been published. Installed builds cache
nothing, but a version number that means two different binaries makes every
later bug report unanswerable.

### Crash-Reporting Dashboard

- [ ] Verify ingestion of new crash reports (if configured).
- [ ] Check for P0/P1 crash clusters.
- [ ] Confirm opt-in consent flow is working.
