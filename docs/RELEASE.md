# Release Process

> **Maintainer runbook.** Step-by-step procedure for cutting a release, including the parts only the maintainer can perform (signing secrets, updater keys, rollback). Contributors want [`releasing.md`](releasing.md) instead. Where this file and `.github/workflows/release.yml` disagree, the workflow is authoritative — it is what actually runs.

> Applies to [Sotto](https://github.com/stofll/Sotto) — the native Rust/Tauri speech-to-text app (**Sotto**).

## Pre-release

### CPU instruction baseline

Whisper's ggml CPU backend uses AVX2, FMA and F16C on x86-64, with AVX-512, AVX-VNNI and AMX disabled. Windows x64 therefore requires a CPU supporting those baseline instructions. On macOS arm64 the compiler's target default applies, without detecting extensions on the build host. GPU builds retain this CPU baseline because some operations still run on the CPU.

The repository's `.cargo/config.toml` selects `scripts/ggml-baseline.cmake` for both local and CI builds. Do not override `CMAKE_TOOLCHAIN_FILE` for distributed binaries unless the replacement preserves this baseline. Host-native compilation can otherwise produce illegal-instruction crashes on a different machine, including when reusing cached builds. After changing the baseline file, rebuild `whisper-rs-sys` with `cargo clean -p whisper-rs-sys` (and `cargo clean --release -p whisper-rs-sys` for release builds); Cargo does not track edits inside a CMake toolchain file. CI cache keys include the baseline files so old native artifacts are not restored. `node scripts/check-ggml-baseline.mjs <cargo-target-dir>` checks the generated CMake caches and fails if a compiled variant does not use the baseline.

### 1. Prepare the version in GitHub Actions

Keep the application version unchanged during normal development. Once the intended changes are merged, open **Actions → Prepare Release → Run workflow**, leave the branch set to `main`, and select `patch`, `minor`, or `major`. An optional exact stable version such as `0.1.0` overrides that selection; enter it without a `v` prefix.

| Selection | From `0.0.5` |
|---|---|
| `patch` | `0.0.6` |
| `minor` | `0.1.0` |
| `major` | `1.0.0` |
| Exact version `0.2.0` | `0.2.0` |

The automatic bump starts from the greatest of the checked-in version and existing stable `vX.Y.Z` tags. Tags for unfinished drafts reserve their numbers too. An exact version must be greater than that baseline; this workflow accepts stable versions only.

Prepare Release reuses successful Rust CI and UI tests for the selected source tree. It accepts checks on the source commit itself or on the head of its merged PR when the resulting trees are identical. It does not create a version PR or repeat full application CI.

The workflow runs the release-script tests, updates the version, and verifies the committed diff contains only the expected version replacements, with dependencies and file modes unchanged. The release bot pushes the new `main` commit and its tag atomically. The tag starts the release build, which creates a draft; publishing does not change the version.

The workflow updates these four sources together without updating dependencies:

| File | Field |
|---|---|
| `desktop/src-tauri/Cargo.toml` | package `version` |
| `desktop/src-tauri/Cargo.lock` | version of the `sotto` package |
| `desktop/package.json` | `version` |
| `desktop/src-tauri/Info.plist` | `CFBundleShortVersionString` |

`tauri.conf.json` has no version override, so Tauri uses `Cargo.toml`. `scripts/check-version.sh` verifies agreement in PR CI and before release builds. There is no need to edit these files or create the tag manually for the normal release path.

#### Repository permissions

Create a private GitHub App, install it only on Sotto, and grant it **Contents: Read and write**. Store its App ID in the Actions variable `RELEASE_APP_ID` and its PEM private key in the Actions secret `RELEASE_APP_PRIVATE_KEY`. The pinned `actions/create-github-app-token` action creates a short-lived token scoped to this repository for the push; source CI is read with the regular `GITHUB_TOKEN`.

In **Settings → Rules → Rulesets**, add the App to the PR/required-check ruleset's bypass list with **Always allow**. Keep the deletion and force-push prohibitions in a separate active ruleset with no bypass actors. Bypass permissions apply to an entire ruleset, not to individual rules or version fields; the workflow's diff check enforces the version-only restriction. No permission to create or approve PRs is needed.

The source workflows are listed in `scripts/check-release-source.mjs`. Add new release-gating workflows there when needed. Missing, pending, failed, or cancelled CI prevents the release; an API error also stops preparation. A workflow started outside `main`, or in a fork, skips preparation.

An App token's tag push triggers Release automatically. Do not also call Release from Prepare Release, as that would build the same version twice. The release build uses its regular `GITHUB_TOKEN`, without the App's bypass permission.

#### Failures and retries

- If CI is incomplete, finish it before preparing the release. If the source cannot reuse a merged PR's identical checked tree (for example, after a direct commit), start both **Rust CI** and **UI tests** manually on `main`. The automatic Rust CI run for a push to `main` only builds and warms the cache, so it does not count. Preparation starts neither.
- If `main` changes during preparation, start a new Prepare Release run. The push never force-updates refs: the release commit and tag are either both accepted or both rejected.
- For an invalid version or missing App configuration, fix the reported problem and start again. If the push result was uncertain, inspect `main` and the tag before retrying; an already pushed tag reserves that version.
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

Three different signatures, often confused:

| | Proves | Where it lives | Set up? |
|---|---|---|---|
| **minisign** (updater) | the update artifact was not tampered with and came from the key holder | `TAURI_SIGNING_PRIVATE_KEY` secret; public half in `tauri.conf.json` | yes |
| **Authenticode** (Windows) | the *publisher* is who they claim to be — this is what silences SmartScreen | `WINDOWS_CERT_BASE64` secret | **no** |
| **codesign** (macOS) | the update is the same application macOS already granted the microphone and Accessibility to | `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` secrets | when the secrets exist |

The macOS row is not about trust. TCC identifies an application by its signature, and the ad-hoc signature in `tauri.macos.conf.json` is identified by the code hash, so without those secrets every release is a new application: the microphone is asked for again and Accessibility stops working with its switch still on. Any certificate that stays the same across releases fixes that, including the self-signed one [scripts/macos-signing.py](../scripts/macos-signing.py) creates for local builds — export its `identity.p12` as base64 into `APPLE_CERTIFICATE` and its common name into `APPLE_SIGNING_IDENTITY`. The release workflow imports it into a temporary keychain before building and warns, without failing, when the secrets are absent.

Replacing the certificate resets those permissions once, for everyone, so treat it the way the minisign key is treated: back it up and keep it.

The three values of the local identity, for the repository's secrets:

```bash
base64 -i ~/.tauri/sotto-local-signing/identity.p12   # APPLE_CERTIFICATE
cat ~/.tauri/sotto-local-signing/password             # APPLE_CERTIFICATE_PASSWORD
openssl x509 -in ~/.tauri/sotto-local-signing/certificate.pem -noout -subject  # APPLE_SIGNING_IDENTITY, the CN
```

Without Authenticode, both the first install and every update show "unknown publisher". The updater works; it just looks untrustworthy. A certificate has not been obtained, so every release so far ships unsigned in the publisher sense.

Until one is, what users get instead is [Verifying a download](verifying-downloads.md): `SHA256SUMS.txt` attached to every release by the `checksums` job, and the minisign key the updater already enforces.

Two conditions worth knowing before shopping, because neither is obvious from a vendor's page:

- **macOS** — Tauri signs and notarizes on its own once the `APPLE_*` variables are present, so the work is a purchase plus secrets. On an individual Developer Program account the certificate carries the maintainer's legal name, and Gatekeeper shows it to users. Developer ID is what removes the quarantine warning; keeping the permissions across updates does not wait for it.
- **Windows** — an EV certificate no longer buys instant SmartScreen trust (Microsoft removed that in 2024); reputation accrues to a consistent publisher identity either way, so the EV premium buys nothing here. Azure Artifact Signing is limited to the US and Canada for individual developers, and an OV certificate now requires a cloud HSM — the private key may not live on the build machine.

- [ ] **Windows Authenticode certificate** loaded in CI secrets (`WINDOWS_CERT_BASE64`, `WINDOWS_CERT_PASSWORD`).
- [ ] **Apple Developer Program** certificate in the existing `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` and `APPLE_SIGNING_IDENTITY` secrets, plus notarization credentials (`APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`).
- [ ] `tauri.conf.json` has `bundle.windows.signing` configured (or a CI override); on macOS the `APPLE_SIGNING_IDENTITY` variable already overrides the ad-hoc identity in `tauri.macos.conf.json`.
- [ ] Test signing locally before tagging: `pnpm tauri build --bundles nsis` (Windows) / `pnpm tauri build --bundles app` followed by `sh scripts/build-dmg.sh` (macOS).

### 6. Bundle Target List

These are the two targets the release workflow builds. On Windows the bundler produces the NSIS installer; on macOS it produces `Sotto.app` and the updater archive, and `scripts/build-dmg.sh` wraps the application into the disk image. There is no MSI and no Intel-Mac build; see [Platform support](platforms.md) for what is promised on each target.

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

Prepare Release pushes its tag with the App token, automatically triggering `.github/workflows/release.yml`. A manually pushed tag also starts that build. It builds Windows and macOS arm64, signs the artifacts, generates `latest.json` and attaches everything to a **draft** release.

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

Without a token no events are queued and neither the delivery worker nor the session watcher starts. Initialization still reads or creates the random installation ID in SQLite.

Settings still shows the toggle as on, so such a build is indistinguishable from a working one until the dashboard stays empty.

The token reaches a build from one of two places, and they must hold the same project token:

| Build | Source |
|---|---|
| tag build in `release.yml` | repository secret `SOTTO_POSTHOG_API_KEY`; verify its presence in the repository's release configuration |
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

The Windows wrapper verifies and stages the pinned Sherpa runtime before compiling, including on a clean checkout, and uses the CLI from `desktop/pnpm-lock.yaml`. The release SBOM includes `native-components.json`: archive URLs and SHA-256 pins, the vendored Whisper source version, and the platform-specific ONNX Runtime versions. When updating a native dependency, review `scripts/native-components.json` together with the Cargo and runtime locks; this source inventory does not replace verification of the packaged binaries.

The cost is a second build tree: a release build shares nothing with a plain `cargo build`, and the first one after this change compiles whisper.cpp from scratch.

Keep both Rust path remapping and the MSVC prefix flags until a tested stable replacement covers both sources of embedded paths. A Cargo-only replacement does not remove paths embedded by C/C++ through `__FILE__`.

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
pnpm tauri build --bundles app --target aarch64-apple-darwin
cd ..
sh scripts/build-dmg.sh desktop/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Sotto.app Sotto_X.Y.Z_aarch64.dmg

# Windows x64
cd desktop
pnpm tauri build --bundles nsis --target x86_64-pc-windows-msvc
```

The disk image script applies the [DMG artwork and icon arrangement](installer-design.md#macos-dmg) without Finder automation, so it behaves the same locally and on the release runner. It needs `uv`. Open the produced DMG on a Mac to verify the background, icon alignment and installation before publishing.

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
2. Install each artifact and smoke-test it. Packaging is gated on browser tests of the release-tag commit in production and development modes, but those tests mock the backend. This manual check remains the gate for the actual installer, native windows, audio, hotkeys, clipboard and model loading.
3. Write the release description using the [template below](#whats-new-template). Review merged PRs or `git log --oneline <previous tag>..vX.Y.Z` as source material, then describe the changes in user-facing language.
4. Publish the draft.

Publishing is the release. `latest.json` is served from `releases/latest/download/`, so until the draft stops being a draft no installed copy sees anything; the moment it is published, eligible installed versions are offered the update. The manifest contains the notes captured during the build; the post-update dialog retrieves the published release description as described below.

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

### Post-update release notes

The GitHub Release description is also the source for the installed version’s “What’s new” dialog. Keep it concise, with headings and bullet lists for features and fixes. The dialog renders Markdown headings, lists, emphasis, code, tables and links; links open in the system browser, while images and raw HTML are shown as inert text and never load remote content. Review the description before publishing. No separate in-app changelog or versioned README links need updating.

Before an in-app update, Sotto fetches and caches the published description for offline display after restart; the manifest notes are a fallback if that request fails. Editing a draft description after CI builds it does not rewrite the already-uploaded `latest.json`, so the post-update dialog prefers the description from the exact GitHub tag. Manual and portable upgrades fetch the description for the exact installed tag when no cache exists. Closing the dialog acknowledges that version locally. Failed downloads, missing notes and offline requests do not mark an upgrade as read; retrieval is retried on the next application launch. First launch with this feature establishes a baseline without showing a dialog, including installations upgraded from older builds that did not track the viewed version. Downgrades do not show the dialog.

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

Never re-tag a version that has been published. Installed builds cache release notes by version, and a version number that means two different binaries makes every later bug report unanswerable.

### Crash-Reporting Dashboard

- [ ] Verify ingestion of new crash reports (if configured).
- [ ] Check for P0/P1 crash clusters.
- [ ] Confirm opt-in consent flow is working.
