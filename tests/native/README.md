# Native feasibility experiment

This is an instrumented Windows experiment, not the release-package suite or a completed native E2E gate. No CI job is added. The macOS bench and native input adapter are still pending; macOS has a native tray menu, not the Windows-only `tray-popup` webview.

The optional `native-e2e` Cargo feature enables `tauri-plugin-wdio-webdriver` 1.4.0. It does not add the WDIO frontend/IPC-mocking plugin, change capabilities, or relax CSP. A small W3C HTTP client probes the embedded driver without installing the full WebdriverIO service. This isolates the first compatibility question; choosing the final test runner comes later.

## Isolation

Use the generated Windows Sandbox configuration. It disables network, clipboard sharing, microphone/camera forwarding, printers and vGPU. Only staged binaries/scripts/runtime are mapped read-only; a new empty report directory is the only writable host mapping. The repository, home directory, live app data and credentials are not shared.

The guest launcher refuses an ordinary host username, existing Sotto processes and existing guest app data. That username check is an accidental-launch guard, not a security boundary; isolation comes from Windows Sandbox. It creates `config.json` with `telemetry_enabled: false` before launching Sotto, and uses guest-only database, log and model paths. This experiment intentionally cannot verify recording or GPU behavior.

## Prepare the build

Use the pinned Node/pnpm and Rust versions and the native prerequisites in [Development](../../docs/development.md). Prepare the verified Sherpa runtime and DLLs as described there and in Rust CI. Neither command below launches the app.

From `desktop/`, build the Windows production frontend with `TAURI_PLATFORM=windows` and `TAURI_DEBUG` unset:

```powershell
$env:TAURI_PLATFORM = 'windows'
Remove-Item Env:TAURI_DEBUG -ErrorAction SilentlyContinue
pnpm build
```

From `desktop/src-tauri/`, build a CPU instrumented executable with telemetry tokens absent. Keep this binary out of installers and releases. Use the existing native library preparation script with the matching release profile if DLLs are not staged yet.

```powershell
Remove-Item Env:SOTTO_POSTHOG_API_KEY,Env:POSTHOG_API_KEY -ErrorAction SilentlyContinue
cargo build --locked --release --features native-e2e --target-dir target/native-e2e
```

Use the separate target directory so an instrumented executable never replaces the ordinary `target/release/Sotto.exe`. Point native library preparation at that target directory as well. The feature is off by default. Instrumented builds reject compile-time telemetry tokens. An instrumented executable exits before accessing application state unless `SOTTO_NATIVE_E2E_PORT` is explicitly set; do not set it or launch the executable on the host.

## Package and run in Sandbox

Windows Sandbox requires a supported Windows edition, virtualization and the optional Sandbox component. If `WindowsSandbox.exe` is absent, enable **Windows Sandbox** in “Turn Windows features on or off” using an administrator account and reboot if requested. The scripts do not enable Windows components, create OS accounts, or restart the host.

Supply an installed WebView2 Runtime directory containing `msedgewebview2.exe`, or a prepared Fixed Version runtime. Also supply the x64 `Microsoft.VC143.CRT` directory from the Visual Studio redistributables: the executable imports MSVCP140, which must not be assumed present in a fresh Sandbox. The packager copies these runtime files; it does not download software or reuse the host WebView profile. This local payload is not a release artifact, and providing its app-local CRT does not verify installer prerequisites.

From the repository root, with the pinned Node executable:

```powershell
node --test tests/native/sandbox.test.mjs
node tests/native/package-sandbox.mjs desktop/src-tauri/target/native-e2e/release "C:/path/to/WebView2/Application/version" "C:/path/to/VC/Redist/MSVC/version/x64/Microsoft.VC143.CRT" test-results/native/new-run
```

The output directory must not exist. The executable must have the instrumented-build marker, the four Sherpa runtime DLLs must be beside it, and `portable.flag` is refused. The marker is a packaging guard; the manifest's SHA-256 identifies the actual tested files, not a verified source commit. Record the source revision and dirty-tree state with the build separately until build provenance is automated.

Open the generated `native-spike.wsb`. The guest seeds a light theme, starts Sotto, verifies the settings UI and unchanged CSP, reads the WebView runtime version, takes a screenshot and clicks a real UI disclosure. It then observes window handles for up to 120 seconds. During that interval, left-click the Sotto icon in the guest tray, then right-click it and choose **Exit**. This native tray action is assisted during the feasibility experiment; it is never replaced by an IPC call.

Read `reports/report.json`, `main.png`, optional `tray.png`, `app.log`, `stdout.log`, `stderr.log` and `probe.log`. `exit-code.txt` records the probe result. Exit code 1 means failure; 2 means partial evidence. No result from this spike is an E2E pass: overlay display/focus, native input automation and frontend error collection remain unverified even when DOM interaction works. A zero Sotto exit is recorded separately and needs confirmation that the native Exit action was used.

If the application does not exit, the launcher terminates only its own tracked child and marks forced cleanup. Closing Sandbox discards guest state; the dedicated reports remain on the host. A missing report or skipped observation is not success. Preserve the failed run before preparing a fresh directory.

## Remaining experiment boundaries

The embedded driver sees webviews, including hidden ones. Readable overlay DOM is not proof that the native overlay can be shown, placed or focused correctly. On macOS, the corresponding tray test needs native menu automation and must not expect a third webview.

Backend diagnostics use the existing log files. The client does not add a runtime diagnostics IPC or install mock/event hooks; complete frontend error capture has not been established. CSP failures are reported without bypassing the policy.

The unchanged release package requires a separate OS-level suite. Tests of the instrumented binary cannot establish installer, updater signature, native microphone, paste or release-package correctness. See the [testing guide](../../docs/testing.md).

Sources: [Windows Sandbox](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/), [Sandbox configuration](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-configure-using-wsb-file), [Tauri WebDriver](https://v2.tauri.app/develop/tests/webdriver/).
