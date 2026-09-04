# Development

## Prerequisites

Use the same platform you intend to test on. Native audio, window, and bundle
behavior is not fully portable between operating systems.

Common to all platforms:

- **Rust** stable toolchain — <https://rustup.rs>. The version is pinned by
  `rust-toolchain.toml` at the repository root.
- **Node.js** LTS (20+) and **pnpm** (`npm i -g pnpm` or via Corepack)
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

The frontend-only development server is available with `pnpm dev`. A full
Tauri build also requires the native prerequisites and model/runtime assets
described in [Models](models.md).

On Windows there is also a launcher: `desktop\run_desktop.cmd`.

## Build the desktop app

```bash
cd desktop
pnpm tauri build
```

Windows artifacts land in `desktop\src-tauri\target\release\`:

- Installer: `bundle\nsis\Sotto_<version>_x64-setup.exe`
- Direct executable: `Sotto.exe`

Use the installer for a normal install; the direct executable is handy for a
quick local check without installing. On Windows, prefer the scripted build,
which sets up the MSVC / CMake / LLVM environment for you:

```cmd
scripts\build-installer.bat
```

## Repository layout

```text
desktop/
  src/            React/TypeScript frontend — one entry point per window
  src-tauri/      Rust backend (Tauri commands, whisper engine, audio, DB, AI)
docs/             public user, contributor, privacy, and release docs
scripts/          build + release helpers
```

[Architecture](architecture.md) describes the boundaries these directories
implement.

## Pinned GitHub Actions

Every `uses:` in `.github/workflows/` is pinned to a full commit SHA, with the
version it corresponds to in a trailing comment. A tag such as `v4` is a
mutable reference: the action's owner can move it to a different commit, and
that commit runs with our workflow token. A SHA cannot be moved. The
`static-gate` job in `rust-ci.yml` fails if an unpinned action appears.

To update one:

```bash
# Resolve the tag you want to move to.
gh api repos/<owner>/<repo>/commits/<tag> --jq .sha
```

Replace the SHA in the workflow and update the trailing comment to the tag you
just resolved. Read the action's changelog between the old and new version
before you do — a pin exists so that a new version is a decision, not an
event. `dtolnay/rust-toolchain` publishes no releases, so its pin tracks the
`stable` branch head and its comment records the date it was taken.

## Dependency inventory

`.github/workflows/sbom.yml` produces a CycloneDX SBOM for the Rust and npm
dependency graphs plus a readable license report, and uploads them as a run
artifact. On a release tag `release.yml` calls the same workflow after the
draft exists, so the files land on the release itself.

The Rust SBOM is generated with `--target all --all-features`: most of the
graph arrives through `[target.'cfg(...)']` blocks and optional GPU features,
and a Linux-only inventory would describe a build we do not ship. On the npm
side, platform binaries for operating systems other than the runner's are
listed but carry no license — they are named in the lockfile and never
installed, so there is no manifest to read. The job's summary step prints how
many components lack a license so that number stays visible.

## Working conventions

- Keep user-visible behavior and documentation in sync with the current UI.
- Do not commit API keys, local databases, recordings, build output, or local
  agent state.
- Prefer focused pull requests with tests and documentation for behavior that
  users or contributors must understand.
