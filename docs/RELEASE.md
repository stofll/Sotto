# Release Process

> **Maintainer runbook.** Step-by-step procedure for cutting a release, including the parts only the maintainer can perform (signing secrets, updater keys, rollback). Contributors want [`releasing.md`](releasing.md) instead. Where this file and `.github/workflows/release.yml` disagree, the workflow is authoritative — it is what actually runs.

> Applies to [Sotto](https://github.com/stofll/Sotto) — the native Rust/Tauri speech-to-text app (**Sotto**).

## Pre-release

### 1. Prepare the version in GitHub Actions

Keep the application version unchanged during normal development. Once the intended changes are merged, open **Actions → Prepare Release → Run workflow**, leave the branch set to `main`, and select `patch`, `minor`, or `major`. An optional exact stable version such as `0.1.0` overrides that selection; enter it without a `v` prefix.

| Selection | From `0.0.5` |
|---|---|
| `patch` | `0.0.6` |
| `minor` | `0.1.0` |
| `major` | `1.0.0` |
| Exact version `0.2.0` | `0.2.0` |

The automatic bump starts from the greatest of the checked-in version and existing stable `vX.Y.Z` tags. Tags for unfinished drafts reserve their numbers too. An exact version must be greater than that baseline; this workflow accepts stable versions only.

Prepare Release creates a version-only PR, dispatches Rust CI on its head commit, waits for success, and merges it using the normal branch protections. It then tags the merged commit and calls the release build, which creates a draft. The version is fixed before compilation; publishing the draft does not change it.

The workflow updates these four sources together without updating dependencies:

| File | Field |
|---|---|
| `desktop/src-tauri/Cargo.toml` | package `version` |
| `desktop/src-tauri/Cargo.lock` | version of the `sotto` package |
| `desktop/package.json` | `version` |
| `desktop/src-tauri/Info.plist` | `CFBundleShortVersionString` |

`tauri.conf.json` has no version override, so Tauri uses `Cargo.toml`. `scripts/check-version.sh` verifies agreement in PR CI and before release builds. There is no need to edit these files or create the tag manually for the normal release path.

#### Repository permissions

Enable **Settings → Actions → General → Workflow permissions → Allow GitHub Actions to create and approve pull requests**. The workflows request their own scoped `contents`, `pull-requests`, and `actions` permissions and use `GITHUB_TOKEN`; no personal access token is required. The workflow creates a PR but never approves one or bypasses branch protection.

The current `main` rules require a PR, an up-to-date branch, and the Rust CI checks, with no required approvals. If approvals are introduced later, they must be provided before the merge job can succeed. A workflow started from a branch other than `main`, or in a fork, skips preparation.

Bot-created PRs and tag pushes do not automatically trigger other workflows. Prepare Release explicitly dispatches Rust CI on the release branch so required checks attach to the PR head, then explicitly calls Release after tagging. Keep those explicit invocations if changing the workflow structure.

#### Failures and retries

- For a transient CI or merge error, use **Re-run failed jobs** in the original Prepare Release run. It reuses that run's version and PR. A retry accepts an existing tag only if it points to the same release commit.
- If `main` changes during preparation, or the code needs fixing, close the unmerged release PR, merge the fixes into `main`, and start Prepare Release again. Preparation refuses to create another PR while a `release/` PR is open. Do not manually edit the release PR: the workflow checks and tags the captured commit only.
- If preparation failed before creating a PR, fix the reported permissions or input problem and start a new run. An unused `release/` branch can be deleted after confirming that no run is using it.
- If the release build fails after tagging, rerun its failed jobs or run **Release** manually with the existing tag. The build and SBOM resolve that tag rather than the selected UI branch. Published releases cannot be rebuilt; issue a new version instead.

For local inspection, `sh scripts/check-version.sh [vX.Y.Z]` checks metadata without modifying it. `sh scripts/release.sh` remains an optional dry run for the manual tagging path; it is not a step in automated preparation.

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

If advisories are found, upgrade affected dependencies in a separate PR before proceeding with the release.

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

Without Authenticode, both the first install and every update show "unknown publisher". The updater works; it just looks untrustworthy. A certificate has not been obtained, so every release so far ships unsigned in the publisher sense.

Until one is, what users get instead is [Verifying a download](verifying-downloads.md): `SHA256SUMS.txt` attached to every release by the `checksums` job, and the minisign key the updater already enforces.

Two conditions worth knowing before shopping, because neither is obvious from a vendor's page:

- **macOS** — Tauri signs and notarizes on its own once the `APPLE_*` variables are present, so the work is a purchase plus secrets. On an individual Developer Program account the certificate carries the maintainer's legal name, and Gatekeeper shows it to users.
- **Windows** — an EV certificate no longer buys instant SmartScreen trust (Microsoft removed that in 2024); reputation accrues to a consistent publisher identity either way, so the EV premium buys nothing here. Azure Artifact Signing is limited to the US and Canada for individual developers, and an OV certificate now requires a cloud HSM — the private key may not live on the build machine.

- [ ] **Windows Authenticode certificate** loaded in CI secrets (`WINDOWS_CERT_BASE64`, `WINDOWS_CERT_PASSWORD`).
- [ ] **Apple Developer Program** certificate + notarization credentials in CI secrets (`APPLE_CERT_BASE64`, `APPLE_CERT_PASSWORD`, `APPLE_NOTARIZATION_USERNAME`, `APPLE_NOTarIZATION_PASSWORD`).
- [ ] `tauri.conf.json` has `bundle.windows.signing` and `bundle.macOS.signing` configured (or CI override).
- [ ] Test signing locally before tagging: `pnpm tauri build --bundles nsis` (Windows) / `pnpm tauri build --bundles dmg` (macOS).

### 6. Bundle Target List

These are the two targets the release workflow builds, and the bundle formats `tauri.conf.json` declares (`"targets": ["dmg", "nsis"]`). There is no MSI and no Intel-Mac build; see [Platform support](platforms.md) for what is promised on each target.

| Platform | Bundle format | Target triple |
|----------|---------------|---------------|
| macOS arm64 | `.dmg` | `aarch64-apple-darwin` |
| Windows x64 | `.exe` (NSIS) | `x86_64-pc-windows-msvc` |

---

## Windows Installer

Everything the NSIS installer needs beyond `bundle.windows.nsis` in `tauri.conf.json` lives in `desktop/src-tauri/installer/`:

| File | What it is |
|------|------------|
| `header.bmp` | 150×57, drawn in the wizard header band |
| `sidebar.bmp` | 164×314, the left strip of the welcome and finish pages |
| `installer.nsi` | fork of the bundler's own template |
| `hooks.nsh` | `NSIS_HOOK_*` macros |

**Artwork.** Both bitmaps must be 24-bit BMP at exactly those sizes — MUI2 does not scale them, and a 32-bit BMP (GDI+'s default, alpha channel included) renders as garbage. Regenerate them from the app icon with:

```bash
powershell -ExecutionPolicy Bypass -File scripts/make-installer-art.ps1
```

**Template fork.** `installer.nsi` is a copy of tauri-bundler's template with three marked changes:

- Branded welcome/finish copy as `LangString`s.
- `NOSTRETCH` on both bitmaps.
- The hooks `!include` moved below the `!define` block. Upstream places it above, so a hook referencing `${PRODUCTNAME}` silently gets an empty string.

Every edit is tagged `Sotto:` to make comparison with a new upstream template straightforward.

Pin check: the bundler version is not the CLI version. Read it from the CLI's lockfile, then unpack that bundler to diff its template against ours:

```bash
tar xzf ~/.cargo/registry/cache/*/tauri-cli-<ver>.crate tauri-cli-<ver>/Cargo.lock
```

**Rename migration.** `hooks.nsh` removes an installation made under a previous product name. The uninstall registry key is `Uninstall\${PRODUCTNAME}`, keyed on the display name rather than the bundle id, so after the `Шёпот` → `Sotto` rename the built-in "already installed" check no longer sees the old copy and would leave two of everything.

The hook finds it by `Publisher`, which the bundler fills with the second segment of the identifier, and runs its uninstaller in passive mode, which keeps user data.

Because the identifier moved too (`com.shepot.app` → `com.sotto.app`), that `Publisher` no longer matches the current build: the installed copy is stamped `shepot`, this one is `sotto`.

The hook therefore carries `LEGACYPUBLISHER` and matches either, reading the install directory out of `Software\<publisher>\…` under whichever one hit. Drop the constant when the whole file goes.

It runs on the updater path too (`/UPDATE`), where it matters most: an update from `Шёпот` installs into a new directory, and nothing else would ever clean up the old one.

---

## Updater Keys (one-time setup)

The keypair was generated with:

```bash
pnpm exec tauri signer generate -w ~/.tauri/sotto.key
```

- **Public half** — already committed as `plugins.updater.pubkey` in `desktop/src-tauri/tauri.conf.json`. An installed build refuses any update whose signature does not match it.
- **Private half** — `~/.tauri/sotto.key`, never committed. Its contents go into the repository secret `TAURI_SIGNING_PRIVATE_KEY`, and the password (empty for this key) into `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

Losing the private key means shipped installations can no longer be updated: they will reject artifacts signed by any replacement key, and every user has to reinstall by hand. Back it up somewhere other than this machine.

---

## Tag & Build

Prepare Release creates the tag and calls `.github/workflows/release.yml` automatically. A manually pushed tag also starts that build. It builds Windows and macOS arm64, signs the artifacts, generates `latest.json` and attaches everything to a **draft** release.

Publishing that draft is what makes the update visible to users, so check the build before you press it.

The release body is what the app shows as "what's new", so write it for users rather than as a changelog dump.

### Tag Format

Automated stable releases use `vX.Y.Z` (e.g., `v0.2.0`). The version checker also accepts pre-release tags such as `v0.2.0-rc.1`, but Prepare Release does not manage a pre-release channel. The manual stable tagging fallback requires synchronized versions already committed to `main`:

```bash
# After version bump commit is on main
git tag v0.2.0
git push origin v0.2.0
```

### Telemetry Ingest Token

Release builds compile the public PostHog ingest token in at build time, so a build either has telemetry or does not — the runtime toggle cannot add it back.

Built without the token, telemetry is a complete no-op: nothing is queued, no worker runs, and the linker drops the delivery path out of the binary.

Settings still shows the toggle as on, so such a build is indistinguishable from a working one until the dashboard stays empty.

The token reaches a build from one of two places, and they must hold the same project token:

| Build | Source |
|---|---|
| tag build in `release.yml` | repository secret `SOTTO_POSTHOG_API_KEY` — **set**, so both targets ship with telemetry |
| `build-installer.sh`, outside CI | `~/.tauri/sotto-posthog.key`, next to the updater signing key |

For the local script, override the file with `SOTTO_POSTHOG_KEY_PATH`, or set `SOTTO_POSTHOG_API_KEY` in the environment to win over it.

The local script checks telemetry configuration twice:

- Before `cargo`, it stops if the token is missing.
- After the build, it searches the artifact for the ingest host. This catches a token that was set but never reached `rustc`.

CI has neither guard. The workflow passes the secret straight through, and `option_env!` reads an absent one as `None`, so a renamed, rotated-away or expired secret does not fail anything: the release builds, installs and behaves identically, and simply never reports.

If the dashboard goes quiet after a release, suspect the secret before the client.

Pass `SOTTO_ALLOW_NO_TELEMETRY=1` to build deliberately without telemetry; it skips both the pre-build guard and the artifact check.

Create the local key file once:

```powershell
Set-Content -Path $env:USERPROFILE\.tauri\sotto-posthog.key -Value "phc_..." -Encoding ascii
```

The token is the public project ingest key, never a personal or administrative PostHog key. See [telemetry.md](telemetry.md).

### Build Paths

A release binary must not carry the build machine's directory layout. Two different tools bake it in, and each needs its own countermeasure:

| Source | What leaks | Fix |
|---|---|---|
| rustc `file!()` in dependency panic messages | `$CARGO_HOME/registry` — the **OS user name** | `--remap-path-prefix` (`CARGO_ENCODED_RUSTFLAGS` in `build-installer.sh`, `RUSTFLAGS` in `release.yml`) |
| MSVC `__FILE__` in whisper.cpp asserts | the build directory | build outside the working copy (`CARGO_TARGET_DIR`) |

`--remap-path-prefix` is an rustc flag and never reaches `cl.exe`; MSVC has no `-ffile-prefix-map`, only the undocumented `/d1trimfile:`.

So instead of a flag, the release build moves the target directory itself: whisper.cpp is unpacked into `OUT_DIR` and travels with it.

`scripts/build-installer.sh` sets `CARGO_TARGET_DIR` to `<repo drive>:/sotto-build`; override with `SOTTO_BUILD_DIR`. The bundle therefore lands in `$CARGO_TARGET_DIR/release/bundle/nsis`, **not** under `desktop/src-tauri`.

The cost is a second build tree: a release build shares nothing with a plain `cargo build`, and the first one after this change compiles whisper.cpp from scratch.

The profile-level `trim-paths` would replace the remap declaratively, but it still requires nightly in Cargo 1.95. Swap it in when it stabilises.

Verify any binary you are about to hand out:

```bash
python scripts/check-build-paths.py <path-to-exe>
```

It runs automatically at the end of `build-installer.sh` and as a step in `release.yml`. In CI the step runs *after* the artifacts are uploaded — the release is a draft, so a red check is a reason not to publish it.

### Build Commands

Windows releases go through the wrapper, not `pnpm tauri build` directly — it is what exports the signing password and the path flags above:

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

The `checksums` job in `release.yml` does this: after both builds and the SBOM land in the draft, it downloads the draft's own assets, hashes them and uploads `SHA256SUMS.txt`.

Hashing the release rather than the build directory is the point — it verifies what people will actually download.

Manually, from a local build:

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

The release itself already exists by this point: `tauri-action` opened it as a draft when the tag build started, and the `sbom` and `checksums` jobs added their assets to it. Nothing here creates a release or uploads an asset.

1. Open the draft and check the assets against [Draft contents](#draft-contents) below.
2. Install each artifact and smoke-test it. This is the only gate between the build and every existing installation.
3. Write the release description using the [template below](#whats-new-template). Review merged PRs or `git log --oneline <previous tag>..vX.Y.Z` as source material, then describe the changes in user-facing language.
4. Publish the draft.

Publishing is the release. `latest.json` is served from `releases/latest/download/`, so until the draft stops being a draft no installed copy sees anything; the moment it is published, every 0.x install is offered the update with this body as its "what's new".

### What's New Template

Write release notes in English. Start with the main benefit, then describe observable changes in short bullets. Link a PR when it provides useful detail; avoid copying commit titles or internal implementation summaries.

The description also appears in Sotto's update dialog. Keep it concise, and remove empty sections, placeholder text, and authoring comments before publishing. A small patch may need only a short introduction and a `Fixed` section.

```markdown
<!-- Briefly describe the main benefit of this release. -->

## New and improved

- [Describe a new capability or improvement and where to use it.]

## Fixed

- [Describe the affected scenario and the corrected behavior.]

## Update notes

<!-- Include required actions, compatibility changes, or known limitations only when relevant. Otherwise remove this section. -->

[Full changelog](https://github.com/stofll/Sotto/compare/PREVIOUS_TAG...CURRENT_TAG)
```

Replace the changelog placeholders with the actual previous and current tags. Add a `Security` or `Performance` section only when it helps explain substantive changes; never fill an empty category with "None in this release".

Keep required upgrade actions near the top if they affect whether a user should install. For a major visual feature, a screenshot or short demo can accompany the explanation; essential instructions must also be readable as text.

### Draft contents

Everything below is uploaded by CI. A missing entry means a job failed or was skipped, and is a reason to fix the build rather than to upload by hand.

| Asset | From |
|---|---|
| `Sotto_X.Y.Z_aarch64.dmg` | `release` job, macOS |
| `Sotto_aarch64.app.tar.gz` + `.sig` | `release` job, macOS — the updater artifact |
| `Sotto_X.Y.Z_x64-setup.exe` + `.sig` | `release` job, Windows |
| `Sotto-<tag>-windows-x64-portable.zip` | `release` job, Windows — manual-update portable build |
| `latest.json` | `release` job (`includeUpdaterJson`) — the updater manifest |
| `sbom-rust.cdx.json`, `sbom-npm.cdx.json` | `sbom` job |
| `licenses-npm-prod.json`, `licenses-npm-dev.json` | `sbom` job |
| `SHA256SUMS.txt` | `checksums` job, hashed over the draft's own assets |

The source archives GitHub attaches on its own appear only once the draft is published, and are not covered by `SHA256SUMS.txt`.

---

## Post-release

### Monitoring

- Check crash-reporting dashboard (once configured) for new crash clusters within 24 hours.
- Monitor GitHub Issues for installation or runtime reports.
- Verify download counts and asset availability on the Release page.

### Rollback Procedure

The endpoint is `releases/latest/download/latest.json`, so "latest" is whichever release GitHub currently marks as latest — that is the lever.

1. Mark the bad release as a pre-release (or delete it). GitHub then points "latest" at the previous release, and its `latest.json` takes over.
2. Users who have not updated yet see nothing at all.
3. Users who already updated are **not** downgraded automatically: the plugin compares versions and treats an older manifest as "no update available". They need the previous installer by hand, or a `vX.Y.Z+1` that reverts the change — the second option is usually the honest one.

Never re-tag a version that has been published. Installed builds cache nothing, but a version number that means two different binaries makes every later bug report unanswerable.

### Crash-Reporting Dashboard

- [ ] Verify ingestion of new crash reports (if configured).
- [ ] Check for P0/P1 crash clusters.
- [ ] Confirm opt-in consent flow is working.
