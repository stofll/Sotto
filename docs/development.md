# Development

## Prerequisites

Use the same platform you intend to test on. Native audio, window, and bundle behavior is not fully portable between operating systems.

Common to all platforms:

- **Rust** stable toolchain — <https://rustup.rs>. The version is pinned by `rust-toolchain.toml` at the repository root.
- **Node.js** from [`.node-version`](../.node-version) and **pnpm** from the `packageManager` field in [`desktop/package.json`](../desktop/package.json).
- **CMake** — required by the `whisper-rs-sys` build script
- **LLVM / libclang** — required by `bindgen` (set `LIBCLANG_PATH`)

Windows also needs:

- **Visual Studio Build Tools 2022** (MSVC, `vcvars64.bat`)
- **NSIS 3.x** for the installer (`winget install NSIS.NSIS`)
- WebView2 runtime (preinstalled on Windows 11)

macOS also needs:

- **Xcode Command Line Tools** (`xcode-select --install`)

## Run locally

From the repository root:

```bash
cd desktop
pnpm install --frozen-lockfile
pnpm tauri dev
```

On macOS, `pnpm tauri dev` runs `target/debug/Sotto` directly rather than an application bundle, which matters for anything that needs Accessibility — the hotkey and automatic paste. That binary is a separate TCC client from an installed `/Applications/Sotto.app`, and macOS attributes its request to the process that launched it, so the grant has to go to the terminal you run the build from. Granting it to the installed copy does nothing for a development run. Builds are signed ad hoc, which also means every rebuild changes the code hash and silently invalidates an existing grant while the switch in Settings stays on; `tccutil reset Accessibility com.sotto.app` clears the stale entry for the bundle, and [a stable identity](#keep-the-macos-permissions-across-builds) stops it from going stale at all.

The frontend-only development server is available with `pnpm dev`, but application commands require Tauri; there is no HTTP backend fallback. Use the isolated [browser UI harness](ui-testing.md) to exercise synthetic states without launching the native application. A full Tauri build also requires the native prerequisites and model/runtime assets described in [Models](models.md).

On Windows there is also a launcher: `desktop\run_desktop.cmd`.

### Prepare native dependencies for direct Cargo checks

Build the frontend first with `pnpm --dir desktop build`. On Windows, run the following from the repository root in a PowerShell session with MSVC, CMake and libclang available:

```powershell
$env:SHERPA_ONNX_LIB_DIR = ./scripts/fetch-sherpa-runtime.ps1 -Target win-x64-shared
Set-Location desktop/src-tauri
cargo build --locked -p sherpa-onnx-sys
if ($LASTEXITCODE -ne 0) { throw "Sherpa runtime build failed" }
if (-not (Test-Path target/debug/sherpa-onnx-c-api.dll)) {
    cargo clean -p sherpa-onnx-sys
    cargo build --locked -p sherpa-onnx-sys
    if ($LASTEXITCODE -ne 0) { throw "Sherpa runtime rebuild failed" }
}
$env:TAURI_ENV_DEBUG = "true"
./prepare-native-libs.ps1
```

Keep this session for the Cargo checks so `SHERPA_ONNX_LIB_DIR` remains set. The fetch script verifies the archive against `scripts/sherpa-runtime.lock`; staging supplies Tauri's resource glob and the DLLs needed by test executables. A cached Cargo build can lack those DLLs, which is why the missing-file branch rebuilds the native dependency. Plain Cargo does not run Tauri's bundling hook.

On macOS arm64, prepare the verified static runtime from the repository root with `export SHERPA_ONNX_LIB_DIR="$(sh scripts/fetch-sherpa-runtime.sh osx-arm64-static)"` before Cargo checks. The executable preparation steps and platform matrix live in [Rust CI](../.github/workflows/rust-ci.yml).

## Build the desktop app

```bash
cd desktop
pnpm tauri build
```

Windows artifacts land in `desktop\src-tauri\target\release\`:

- Installer: `bundle\nsis\Sotto_<version>_x64-setup.exe`
- Direct executable: `Sotto.exe`

Use the installer for a normal install; the direct executable is handy for a quick local check without installing. For the Windows maintainer build, invoke the wrapper from Git Bash; it prepares signing and telemetry inputs before calling the batch script that sets up MSVC / CMake / LLVM:

```bash
scripts/build-installer.sh
```

That wrapper uses a build directory outside the checkout by default and prints the artifact path. See [Release process](RELEASE.md) for its required signing and telemetry inputs; calling `build-installer.bat` directly is rejected.

The wrapper verifies the pinned Sherpa archive, builds its release runtime and stages the DLLs before compiling the application. It also repairs a warm Cargo cache whose runtime DLLs were removed. Install the desktop dependencies with `pnpm install --frozen-lockfile` first: the wrapper uses the repository's locked Tauri CLI.

On macOS `pnpm tauri build` produces the application in `desktop/src-tauri/target/release/bundle/macos/Sotto.app`, together with the updater archive. The disk image is a separate step that needs `uv` and no Finder automation permission:

```bash
sh scripts/build-dmg.sh desktop/src-tauri/target/release/bundle/macos/Sotto.app Sotto.dmg
```

### Keep the macOS permissions across builds

An ad-hoc signature identifies an application by its code hash, so every build is a different application to macOS: the microphone and Accessibility grants given to the previous one no longer apply, and the Accessibility switch stays on while the paste silently fails. Signing every build with the same certificate keeps both, because the requirement TCC records then names the certificate instead of the hash.

Create the local identity once, from the repository root, and back up the directory it reports. Regenerating it is the same event as never having had it: every grant tied to the old certificate is lost.

```bash
python3 scripts/macos-signing.py init
```

Then sign each build before installing it. The disk image is packed from the ad-hoc application, so build the application on its own and sign that copy:

```bash
pnpm --dir desktop tauri build --bundles app
python3 scripts/macos-signing.py sign desktop/src-tauri/target/release/bundle/macos/Sotto.app
```

The first build signed this way is still a new application to macOS. Remove the stale entry under **Privacy & Security → Accessibility** and add the signed copy once; later builds keep the grant. The certificate is self-signed and trusted by nothing outside the machine, so Gatekeeper treats the build exactly as it treats an ad-hoc one.

## Content Security Policy

`app.security.csp` in `desktop/src-tauri/tauri.conf.json` is the policy every window loads under. It is a single JSON string, so the reason for each relaxation is recorded here rather than next to the directive.

`style-src 'self' 'unsafe-inline'` supports inline styles in development and documents without a style nonce. In packaged windows, Tauri adds nonces to the `<style>` blocks in the built HTML and to `style-src`; the browser then ignores `'unsafe-inline'` for that directive. Dynamically created stylesheets in those windows must reuse an authorized style element's `.nonce` property. Read the property rather than `getAttribute("nonce")`, whose value browsers hide. React's direct style-property updates remain available.

The configured `script-src` stays closed at `'self'`, with no `'unsafe-inline'`, `'unsafe-eval'`, or additional host. Tauri may add authorization for bundled scripts during packaging. The frontend ships as local bundles and evaluates no strings, so an exemption there would only widen what an injected script could reach; treat any proposal to relax this directive as a security change requiring review, not as a build fix.

`default-src 'self'` with `connect-src 'self' ipc: http://ipc.localhost` is what lets a window reach the Rust commands and nothing else — network access belongs to the Rust side and is documented in [Privacy](privacy.md). `img-src` adds `asset:` and `http://asset.localhost` for Tauri's asset protocol and `data:` for inline images.

All three windows load this one policy, so a directive relaxed for one of them is relaxed for the settings window as well.

## Window capabilities

Every application command is declared in `APP_COMMANDS` in `desktop/src-tauri/build.rs`, so Tauri checks each call against the calling window's capability in `desktop/src-tauri/capabilities/`. A window can call only the commands its file grants as `allow-<command-name>`; any other call fails with "not allowed by ACL". The settings window, the overlay and the tray popup each have their own file.

When you add a command, register it in `generate_handler!`, declare it in `APP_COMMANDS`, and grant it in the capability of every window that calls it. `desktop/src/bridge/window-capabilities.test.ts` follows each window's imports from its entry point and fails when a grant is missing or no longer used.

Links leave the app only through the `open_url` command, which accepts web pages and, on macOS, the Privacy & Security panes. The check runs in Rust, whatever the calling window sends.

## Repository layout

```text
desktop/
  src/            React/TypeScript frontend — one entry point per window
  src-tauri/      Rust backend (Tauri commands, whisper engine, audio, DB, AI)
docs/             public user, contributor, privacy, and release docs
scripts/          build + release helpers
tests/ui/         browser UI tests against the Vite entry points
```

[Architecture](architecture.md) describes the boundaries these directories implement, and [Browser UI testing](ui-testing.md) covers the `tests/ui/` suite.

## Pinned GitHub Actions

Every `uses:` in `.github/workflows/` and `.github/actions/` is pinned to a full commit SHA, with the version it corresponds to in a trailing comment.

A tag such as `v4` is a mutable reference: the action's owner can move it to a different commit, and that commit runs with our workflow token. A SHA cannot be moved.

The `static-gate` job in `rust-ci.yml` fails if an unpinned action appears.

To update one:

```bash
# Resolve the tag you want to move to.
gh api repos/<owner>/<repo>/commits/<tag> --jq .sha
```

Replace the SHA in the workflow and update the trailing comment to the tag you just resolved. Read the action's changelog between the old and new version before you do — a pin exists so that a new version is a decision, not an event.

`dtolnay/rust-toolchain` publishes no releases, so its pin tracks the `stable` branch head and its comment records the date it was taken.

## Dependency updates and audit

Dependabot (`.github/dependabot.yml`) proposes monthly Cargo, pnpm, uv and GitHub Actions major and minor version updates, grouping minor releases per ecosystem and limiting each to two open version-update pull requests. Patch-only version proposals are skipped, though an allowed update can still change transitive patch versions in a lockfile. Major upgrades arrive individually for review, and changes to `whisper-rs` and `cpal` also need the benchmark and native microphone checks from [Testing](testing.md). The Sherpa bindings are excluded because they move together with `scripts/sherpa-runtime.lock`, and React minor releases are held while 19.3 would add about 30 kB to every window's startup chunk. Dependabot security updates are separate from this filter, and the weekly security audit continues to check all lockfiles for known advisories.

The lockfile audit lives in `.github/actions/dependency-audit`. The required `Dependency security audit` check runs it on application pull requests, and `security-audit.yml` runs it every Monday, which is what catches an advisory published against an unchanged `main` or the landing page's lockfile.

## Dependency inventory

`.github/workflows/sbom.yml` produces a CycloneDX SBOM for the Rust and npm dependency graphs plus a readable license report, and uploads them as a run artifact.

On a release tag `release.yml` calls the same workflow after the draft exists, so the files land on the release itself.

The Rust SBOM is generated with `--target all --all-features`: most of the graph arrives through `[target.'cfg(...)']` blocks and optional GPU features, and a Linux-only inventory would describe a build we do not ship.

On the npm side, platform binaries for operating systems other than the runner's are listed but carry no license — they are named in the lockfile and never installed, so there is no manifest to read. The job's summary step prints how many components lack a license so that number stays visible.

## Working conventions

- Keep user-visible behavior and documentation in sync with the current UI.
- Do not commit API keys, local databases, recordings, build output, or local agent state.
- Prefer focused pull requests with tests and documentation for behavior that users or contributors must understand.
