# Native automated testing plan for Sotto

Status: a temporary roadmap and hypotheses to validate. Initial Windows scaffolding exists in [tests/native](../tests/native/README.md), but native execution, permissions and tool compatibility remain unverified. Passing harness tests do not establish native E2E coverage.

This plan does not replace [testing.md](testing.md), executable CI configuration, source or manifests. Keep it out of the documentation index. After implementation, move durable isolation, gating and diagnostics rules into maintained guides and remove completed roadmap items.

## Goal and boundaries

Verify critical scenarios in the real application on Windows x64 and macOS arm64. Preserve browser tests, Rust tests and real Whisper/Sherpa checks. Native tests cover OS integration and process lifecycle without duplicating the browser matrix.

The immediate work is the stage 2 experiment after stage 1 isolation. The seven sections are work areas, not seven promised PRs or a linear schedule. Stages 2 and 4 may need several iterations and different tools; determine timing and PR scope from their results.

Out of scope: Linux, macOS x64, every physical microphone and driver, GPU inference and full GPU-release behavior, live cloud accounts, and exhaustive UI permutations. Starting a real release package does not establish Vulkan/Metal correctness. Reintroduce these areas through separate decisions.

## 1. Prerequisites: isolation and inventory

Before launch, prepare a separate OS user or clean VM. Input, clipboard and audio scenarios require an exclusive interactive session without a concurrent personal Sotto installation. A directory or environment variable does not isolate OS input or credential storage.

### Storage

| Data | Current behavior | Test requirement |
| --- | --- | --- |
| Settings and geometry | `config.json` through `app_config_dir()`; adjacent `window.json` | Prepare files in the test user's profile before launch |
| Database, history, statistics and telemetry data | `db::db_path()`: portable `data`, then `SOTTO_CONFIG_DIR`, then `~/.speech_to_text` | Separate database per scenario or snapshot restoration |
| Logs and diagnostic recordings | Nonempty `SPEECH_TO_TEXT_LOG_DIR`, otherwise `db::db_path()/logs`; recordings under the log directory | Account for both overrides; use test audio only |
| Models | Portable `data/models`; otherwise nonempty `SPEECH_TO_TEXT_MODELS_DIR`; otherwise application cache with legacy-cache migration | Shared verified cache is read-only; copy models per scenario when modifying them |
| Credentials | OS credential store, service `speech-to-text` | Separate account and synthetic credentials only |
| WebView | Default WebView data for identifier `com.sotto.app`; paths depend on platform/runtime | Establish actual paths experimentally; portable mode and `SOTTO_CONFIG_DIR` do not establish WebView isolation |

`SOTTO_CONFIG_DIR` also affects logs and recordings through `db::db_path()` unless a separate log path is set. Do not extend this variable to the entire application for tests without a separate product decision.

Portable mode is Windows-only: primary files live in `data` beside the executable, but credentials remain in Windows Credential Manager. Keychain applies to the installed macOS application; this plan assumes no macOS portable package.

Telemetry defaults to enabled. Before launch, write `"telemetry_enabled": false` to the configuration actually used, validating JSON and path. For the offline instrumented build, omit both compile-time token sources: `SOTTO_POSTHOG_API_KEY` and `POSTHOG_API_KEY`. Removing a variable at launch does not remove a compiled token.

An unchanged release package may contain a token and still requires configuration before launch and network restrictions. Absence of a PostHog token disables telemetry but does not rule out other requests; allow only destinations required by the scenario.

`tauri-plugin-single-instance` is registered first and shows the first window on a second launch. A launch can therefore reach another process; differing instance isolation can still leave a global-hotkey conflict. Run ordinary scenarios sequentially in an exclusive session. Test second-instance behavior separately with two test copies.

### Inventory

Record verified values for each machine:

- Machine ID/owner, OS and architecture, WebView2/WKWebView versions, VM image or restoration procedure.
- Runner type, dedicated user, interactive login, session-lock behavior and exclusion of concurrent runs.
- Microphone permission; on macOS, Accessibility for the actual application/tool, Input Monitoring if required by the input method, and screenshot permission if needed.
- Virtual audio versions, routing and installation permissions. Stage 2 may omit audio only for scenarios that do not require it.
- All storage paths, network settings, model/fixture hashes, log collection and normal-exit procedure.
- Assigned machine and evidence of required capabilities for every scenario.

Stop only test-launched processes identified by tracked PIDs. Never reset a personal installation or use it as a fixture. Preserve diagnostics before restoring the environment after failure.

**Readiness:** inventory completed, telemetry disabled before launch, paths verified, and personal data and interactive sessions excluded.

Evidence: [config.rs](../desktop/src-tauri/src/config.rs), [db.rs](../desktop/src-tauri/src/db.rs), [debug.rs](../desktop/src-tauri/src/debug.rs), [model.rs](../desktop/src-tauri/src/model.rs), [telemetry.rs](../desktop/src-tauri/src/telemetry.rs), [portable.rs](../desktop/src-tauri/src/portable.rs).

## 2. Immediate experiment: control the real application

### Two separate environments

| Environment | Application under test | Control and purpose |
| --- | --- | --- |
| A: instrumented native build | Optimized frontend, real Rust backend, embedded WebDriver behind a Cargo feature disabled by default | WebDriver client for short UI smoke tests; evaluate WebdriverIO after transport compatibility is established |
| B: release package | Unchanged installer, portable ZIP or macOS package intended for publication | OS tools for installation, input, window observation and package verification; a separate suite |

A does not verify binary B. B must contain no embedded test server or test commands, and must not be rebuilt or have CSP changed to enable automation. Detecting embedded test automation in a distributed package must fail the check.

WebdriverIO is a candidate, not a proven Sotto solution. The selected free cross-platform approach requires embedded WebDriver on macOS. Official external `tauri-driver` supports Windows/Linux; other options are outside this experiment. Source: [Tauri WebDriver](https://v2.tauri.app/develop/tests/webdriver/).

The WebdriverIO client itself is not prohibited for B: external WebDriver can control WebView2 on Windows. However, A's embedded-driver configuration cannot be reused for B, and native menus, focus and installation still need OS checks. Choose B's tools in a separate experiment. Source: [Microsoft WebView2 WebDriver](https://learn.microsoft.com/en-us/microsoft-edge/webview2/how-to/webdriver).

### Evidence required

- On both systems, launch A with production resources and unchanged CSP, open settings and perform an ordinary UI action. Discover `tray-popup` on Windows; verify the native tray menu through OS tools on macOS, where there is no third webview.
- Find overlay and tray through real activation paths. The overlay is transparent and frameless, and starts hidden on Windows; the tray popup is Windows-only. Do not assume three ordinary WebDriver windows on both systems.
- Verify showing/hiding, content discovery and element accessibility. If showing the overlay requires audio, prepare it or mark the experiment partial.
- Verify `script-src 'self'` and driver operations. An application without `eval` can still encounter driver/CSP incompatibility. Do not add `unsafe-eval` or disable CSP.
- Collect diagnostics and exit normally through the UI. Closing settings is not process exit; verify PID termination and a new process on relaunch.
- Record dependency versions and a reproducible procedure for each machine.

Prohibit IPC mocks, replacement of `invoke`, synthetic application events and direct backend commands in place of user actions. Allow launch/exit, input, UI-state assertions, screenshots and existing diagnostics. Do not enable WebdriverIO mocking features in native scenarios.

Use existing backend logs and capture/paste timings, plus available stdout/stderr and OS crash reports. Validate frontend console/page-error access separately. Do not assume all JavaScript errors reach `app.log`; record diagnostic gaps. Do not add a test IPC channel for errors.

**Success:** launch, settings, overlay/tray discovery and interaction, available logs and normal exit are demonstrated on both systems with unchanged CSP and real IPC. Attach per-machine results and diagnostic limitations.

**Decision:** proceed, change tools or record a blocker. Unsupported overlay/tray or CSP behavior must not be bypassed through test commands. Do not expand the suite, open a series of PRs or add a required CI gate before establishing a viable foundation.

## 3. Short native smoke suite

After the experiment succeeds, cover boundaries absent from browser tests:

- Launch with an empty configuration and with a previously saved light theme.
- Switch dark to light to dark, exit normally and fully restart; saved settings and visible UI must agree.
- Open settings from the real tray; show/hide the Windows popup and operate the macOS native menu.
- Verify native dimensions, transparency and focus, including the overlay not stealing focus where it should not.
- Verify visible, usable controls after restart. A live PID and nonempty screenshot do not establish a working interface.

Keep all eight pages, both languages and the full settings matrix in Playwright. Expand native smoke only for a specific OS boundary or regression.

**Readiness:** the blank-screen regression reliably fails an assertion on a visible, usable UI; the fixed build passes, including full exit and relaunch.

## 4. Separate experiment for the complete dictation path

Start after establishing a dedicated interactive machine, virtual audio, permissions and OS input control. Candidates are VB-CABLE on Windows and BlackHole on macOS; verify compatibility, usage terms, installation and routing before selection. Installation is not assumed complete.

Use Windows Notepad and macOS TextEdit in plain-text mode, plus a local browser input without external services. Pin versions and content-verification methods, and disable editor autocorrection that changes expected output. Bind macOS permissions to the actual application and input tool.

Feed a prepared audio file through the OS audio device, start recording with the real global hotkey, transcribe with a real model and read the result from the target editor.

- Check toggle/push-to-talk, Russian/English keyboard layouts and held/released modifiers.
- Check paste, clipboard-only mode, focus changes and repeated recording.
- Check file transcription separately: no focused-window paste or dictation-history entry.
- Assess text quality with fixed audio/model hashes, expected text and predefined tolerance.
- Assess lifecycle separately through paste/history counts, completion/cancellation and availability of the next operation. Text-quality tolerance must not conceal duplicate paste or a stuck session.

Keep the opt-in `native_microphone_repeated_sessions_preserve_duration` test separate. It verifies PCM duration and repeated-session teardown without UI, STT or history. Native E2E does not replace it, and virtual audio does not verify every physical device.

From `desktop/src-tauri/`, after preparing native prerequisites and microphone permission:

```bash
cargo test --locked --lib native_microphone_repeated_sessions_preserve_duration -- --ignored --nocapture
```

**Readiness:** quality and lifecycle assertions pass independently; output is inserted once, history matches the operation and the next recording works. Unstable input, audio or permissions are an environment limitation, not completed E2E coverage.

## 5. Controlled failures and recovery

For each fault, define its scope, evidence that it occurred, teardown restoration and a successful control scenario afterward. Do not change shared caches or another run's devices. Execute system-state mutations exclusively with snapshot restoration.

| Failure | Injection and isolation |
| --- | --- |
| No microphone / permission denied | Disable only the test virtual device or deny permission to the test app in a dedicated VM/session; verify device-open failure and restore state |
| Cannot save | Restrict ACLs/permissions on only the test data directory under a standard user; verify writes are denied, then restore permissions |
| Corrupt/missing model | Modify a separate model copy; preserve the shared verified cache, restore the copy and verify its hash |
| Silence | Feed a fixed silence fixture through the test audio device |
| Cloud STT/LLM failures | Point the real adapter at a controlled HTTP server with synthetic credentials; define timeout, disconnect, authentication failure and malformed responses per scenario |
| Cancellation/concurrent operation | Perform an ordinary UI/hotkey action during an observed stage; the HTTP server may hold a response during a network stage |

An unavailable microphone and absence of signal are different cases. If the OS cannot reliably produce a permission/device failure, mark that case unsupported instead of substituting silence.

For cancellation during local inference, select a sufficiently long fixture and confirm the actual stage. Do not claim that stage was tested when cancellation happened elsewhere. Live cloud services remain a separate optional area.

**Readiness:** the fault occurred, cancelled output was neither pasted nor recorded as successful, the UI reported the problem, and the next operation succeeded after restoration.

## 6. Release packages and updates: environment B

Start after separately selecting OS tools for the unchanged package. Use clean systems without developer tooling or embedded WebDriver.

- Verify initial installation/launch of the Windows installer, portable ZIP and macOS package, including resources and native libraries.
- Start installed and portable Windows test copies in both orders. Ensure no two active recording/hotkey owners, duplicate paste or mixed data. Record intended single-instance redirection/rejection; do not expect independent simultaneous operation.
- After normal exit of the installed copy, launch portable and vice versa. Portable updates must preserve `data`.
- Verify migration with synthetic data from the previous version and preservation of settings/history.
- Verify absence of automation servers and test commands through build-content inspection and connection/invocation attempts. One closed port is insufficient evidence.
- Tie reports to the source commit SHA and hashes of the exact GitHub Release packages. Do not rebuild or modify those files after verification.

Updates through a test channel with a test signature are a separate experiment if they require changing the endpoint or build key. They verify update mechanics, not the unchanged release binary. Preserve signature verification and check positive and negative scenarios separately.

**Readiness:** reports identify exact packages, installation/migration evidence and unverified update behavior. Launching a GPU-capable package does not verify GPU inference.

## 7. Conditional CI integration and flakiness policy

Do not promise native smoke on every PR yet. After the experiment, assign each scenario a runner, permissions, resources and duration. Start with a manually triggered, nonblocking run, then decide on a PR gate.

Do not assume GitHub-hosted runners cannot run any GUI tests, or that a Windows/macOS runner guarantees microphone, hotkey and permission support. Verify the specific image and scenario. Initially plan a dedicated interactive machine for OS input, audio and paste; installer checks need a clean VM with demonstrated interactive or unattended support. Source: [GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

| Suite | Prerequisite |
| --- | --- |
| Short PR smoke | Stages 2-3 demonstrated on assigned machines with adequate observability and stability |
| Extended/nightly checks | Stages 4-5 demonstrated, resources and costs established |
| Checks before publication | B demonstrated; exact packages tested after building and before publication |

Define safe execution of PR code on dedicated runners: disposable environments, no release secrets or personal data, and controlled cleanup. A dedicated machine does not authorize untrusted code in a persistent profile.

Proposed initial rule: at most one diagnostic retry, preserving both the first failure and the retry. A flake requires confirmed environment instability with unchanged code, artifact and fixtures. A successful retry alone does not establish an infrastructure fault.

After two confirmed flakes in the last 20 independent runs of a scenario on one platform, temporarily remove it from the automatic PR gate, assign an owner and issue, and retain a nonblocking run. Do not classify repeatable product bugs this way. Restore the gate after resolving the cause and completing 20 consecutive independent passes without retries; revisit thresholds after the pilot through a separate decision.

Quarantine must not silently turn a critical check green. Reports must expose the gap, and publication requires an equivalent recorded check of the same artifact. A skipped required check remains unverified and blocks its gate.

Record OS/runtime version, machine ID, commit SHA and artifact hashes, failing step, timings, screenshots and available logs. Do not promise frontend logs before access is demonstrated. Use bounded waits tied to observable state.

## Practical next step

Establish stage 1 isolation and inventory, then conduct only the stage 2 experiment on Windows x64 and macOS arm64. Record confirmed, limited and unverified evidence for startup, windows, CSP, logs and exit before choosing the final tool and next PR scope. Existing Windows scaffolding is a starting point, not proof that the experiment passed.

If successful, add stage 3 smoke with the blank-screen criterion. Dictation, controlled failures, release packages and CI require separate decisions after their prerequisites are demonstrated. Missing machines or permissions block the corresponding checks; browser tests cannot substitute for them.

## Related documents

- [Required checks and native scenarios](testing.md).
- [Browser UI tests and coverage boundaries](ui-testing.md).
- [Platform support](platforms.md).
- [Application architecture](architecture.md).
- [Windows portable mode](portable.md).
- [Release process](RELEASE.md).
