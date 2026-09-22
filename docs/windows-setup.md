# Windows setup

Sotto's custom Windows setup opens a small window with the application name, an Install button and slow animated background waves. The original icon stays in the native title bar; installation details open in a popover without scrolling the page. It supports Russian and English, light and dark appearance, keyboard focus and reduced motion. Animation pauses while the window is hidden.

This is a separate Tauri executable. It embeds the existing NSIS package and delegates file replacement, native libraries, shortcuts, previous-name migration and uninstall registration to that package. The setup UI does not load the dictation runtime, application settings, history or speech models. It installs for the current Windows user.

Installation options below Install open a separate view with the full application path, a native folder picker and independent Desktop and Start menu shortcut choices. Back keeps those choices. Both directions use a short fade and movement, disabled when reduced motion is requested.

On a fresh installation the default is the current user's Local AppData folder followed by Sotto, resolved by Windows and displayed as a full path. A custom destination must be an empty local folder; the default folder may keep leftovers of a removed installation. When Sotto is already registered, setup shows its existing directory read-only; moving an installed application is not supported. Setup refuses to replace a newer installed version with an older package, and stops if the registered version cannot be read. Explicitly disabling a shortcut removes an existing Sotto shortcut at the installer-managed location, but preserves a same-named shortcut targeting another application.

## Development and preview

Install frontend dependencies using the pinned Node and pnpm versions in [Development](development.md). From desktop/:

```powershell
pnpm setup:dev
```

Open the loopback address printed by Vite. Browser mode is explicitly marked as a preview and never executes installation commands. The Install button simulates progress; ?demo=error exercises its error screen.

For the native preview, stop the separate setup:dev server first, then run:

```powershell
pnpm setup:native
```

A debug build without an embedded NSIS package is also a preview. A packaged executable can be opened with --preview to inspect its UI without installing or launching Sotto. The setup has its own application identifier, com.sotto.setup; WebView cache is separate from com.sotto.app. No Sotto config, history, recordings or models are opened by preview mode.

## Build a candidate

First build the normal Windows NSIS package through the existing [release wrapper](RELEASE.md#build-commands). Build the custom shell around that exact package from desktop/. Replace the example paths and version with the actual build:

```powershell
$payload = (Resolve-Path 'D:/sotto-build/release/bundle/nsis/Sotto_0.1.3_x64-setup.exe').Path
$digest = (Get-FileHash -LiteralPath $payload -Algorithm SHA256).Hash
pnpm setup:package --payload $payload --sha256 $digest --output 'D:/sotto-build/Sotto_0.1.3_x64-setup-ui.exe'
```

The build rejects a version/architecture filename mismatch, an invalid digest, a non-executable payload, an existing output file and a payload without the SottoSetupOptionsV1 capability marker. Rebuild NSIS from current source before packaging this shell: previously published packages cannot honor the independent shortcut choices. For a downloaded release, use the published digest rather than treating a freshly computed digest as authenticity verification. The setup version is read from desktop/package.json; there is no second release version to bump.

The embedded payload is checked again before extraction to a unique temporary directory. Release builds require a payload. The shell and its frontend are built separately from the speech application; no native audio toolchain is needed to rebuild only the shell around a verified package.

The new executable is currently a candidate artifact. Release automation and the download site still distribute the established NSIS installer. Keep its .sig and latest.json unchanged: existing clients must continue receiving the original updater-compatible NSIS package. Publisher signing of the new shell is separate from the updater's minisign signature and must be addressed before changing the public download.

The change that starts publishing the shell must also add it to the release SBOM. Its Rust dependencies come from its own `desktop/setup/src-tauri/Cargo.lock`, which `sbom.yml` does not read yet; generate a separate CycloneDX file for it, such as `sbom-setup-rust.cdx.json`, and add that lockfile and its `Cargo.toml` to the workflow's trigger paths. Drive the Rust step from a list of projects, one inventory per distributed binary, so a future macOS setup shell is a single new entry. The shell's frontend is already in the npm inventory, and `cargo-audit` already checks its lockfile.

## Runtime behavior and limitations

The custom UI reports preparation and installation with an indeterminate progress indicator. It does not infer percentages from time or parse localized installer logs. On success, Launch Sotto resolves the installed executable from the per-user uninstall registration and checks the expected version before opening it.

Once file replacement begins, closing the setup is blocked until the child installer finishes. The window may be minimized. A failed attempt offers Retry; the wrapper does not promise transactional rollback. Forced termination of Windows or the process can still interrupt NSIS. Close Sotto before retrying a failed installation.

If WebView2 cannot be detected, a native message explains that the standard NSIS interface will open. That interface can provision WebView2 using its existing policy. No additional download endpoint is introduced by the custom shell. An offline first install on a machine without WebView2 retains the limitations of the embedded NSIS package's runtime-delivery mode.

For now, uninstalling continues through Windows Installed apps and the existing NSIS uninstaller. macOS keeps the drag-to-Applications disk image described in [Installer design](installer-design.md#macos-dmg).

## Verification

```powershell
# From desktop/
pnpm setup:build
pnpm setup:check
pnpm i18n:check
pnpm test

# From desktop/setup/src-tauri/
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked

# From the repository root
node --test scripts/build-setup.test.mjs
uv run --locked --project tests/ui pytest tests/ui/test_installer.py --browser chromium
```

The UI tests use synthetic IPC and isolated browser profiles. Rust tests cover state transitions, duplicate requests, directory validation and payload integrity; they do not install the application.

On Windows, scripts/test-setup-options.py --makensis <path-to-makensis.exe> --utils <path-to-generated-utils.nsh> compiles a harmless NSIS fixture using the production shortcut functions and the pinned Tauri bundler's generated helpers. It runs the ignored native_nsis_options Rust test through the real command invocation, checks paths with spaces and Cyrillic, all four shortcut combinations, opt-outs on reinstall, preservation of unrelated shortcuts, standard defaults and invalid flags. All files and shortcuts are confined to temporary directories; no application registration is written.

Before replacing the public installer, verify a real fresh installation, reinstall, previous-version update, previous-name migration, no-WebView2 fallback, offline failure, running-app handling and uninstall on isolated Windows 10/11 machines. Confirm user data survives and both Whisper and Sherpa load from the installed package.
