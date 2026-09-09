# Testing

Run the frontend checks from the repository root:

```bash
cd desktop
pnpm install --frozen-lockfile
pnpm exec tsc --noEmit
pnpm test
pnpm i18n:check
pnpm build
pnpm bundle:check
```

`pnpm bundle:check` reads the built `dist/` and compares each window's startup JavaScript against a budget declared in `desktop/check-bundle-size.mjs`. Vite's own 500 kB notice only warns; this step fails.

Raising a budget is a deliberate decision, not a way to make the check quiet.

Run the Rust checks from the repository root:

```bash
cd desktop/src-tauri
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Build the frontend before Cargo checks: Tauri reads `desktop/dist`. On Windows, prepare the verified Sherpa runtime before direct Cargo commands using the steps in [Development](development.md) and [Rust CI](../.github/workflows/rust-ci.yml); plain Cargo does not run the bundling hook. Use CPU defaults unless testing the appropriate platform's GPU feature.

The CI workflow is authoritative for the operating-system matrix. Run relevant checks locally before a pull request; CI repeats them on clean runners. Local success does not replace CI, and CI does not replace native verification.

## Prepared speech and real models

Every PR runs deterministic tests without speech downloads, plus an explicitly selected real Whisper CPU smoke test on Windows, macOS and Linux. Windows and macOS also run the Sherpa smoke test. The ignored marker keeps network/model downloads out of an ordinary local `cargo test`; the CI commands explicitly include those tests.

Reproduce the model tests locally from `desktop/src-tauri` when changing capture, resampling, model loading or inference:

```bash
cargo test --locked --test test_whisper_engine -- --ignored --nocapture
# Windows and macOS:
cargo test --locked --test test_sherpa_runtime -- --ignored --nocapture
```

These tests download verified weights into isolated temporary directories. Their public speech fixtures are pinned by source revision and SHA-256 in the test sources. Whisper tiny checks two recognizable English phrases from the upstream JFK sample and repeated inference with the same state. Sherpa checks loading, silence, reset/reload and the final word of a Russian streaming sample. These are inference regressions, not a broad accuracy benchmark or the full hotkey-to-paste flow.

When expanding accuracy coverage, use a reviewed corpus with expected transcripts and explicit word-error tolerances per model/language. Include silence, short speech, noise and trailing speech. Keep this separate from deterministic timeout, cancellation and lifecycle tests so a recognition-quality change cannot hide a broken session transition.

## Native verification before release

Changes to microphone capture, global shortcuts, clipboard behavior, installers, or model loading require checks on each affected OS with a packaged application. Establish where configuration, history, recordings and models will be stored before launching it, and isolate test data from the developer's live data. Never reset live data to prepare a test.

- Start/stop through the hotkey and tray, in toggle and push-to-talk modes. Repeat immediately after silence, cancellation and a recoverable failure.
- Cancel during capture, inference and optional LLM processing. Verify that cancelled text is neither inserted nor written as a successful history entry.
- Test 44.1 kHz and 48 kHz input where available, microphone disconnect and device-open failure. Verify the next recording still works.
- Verify paste in a native text editor and a browser input, with Latin and Cyrillic layouts. Test held/released hotkey modifiers, focus changes, clipboard-only mode and manual paste after an automatic-paste failure. On macOS this requires Accessibility access for the actual installed build.
- Transcribe a file while an editable field is focused: no automatic paste or dictation history entry. Competing microphone/file jobs must be refused without interrupting the active job.
- Exercise local Whisper and supported Sherpa models, configured cloud STT and optional LLM formatting. Verify timeout/error fallback with a controlled endpoint. Check packaged GPU inference on the applicable release target.

Record the OS, application build, cases actually run and limitations. A compile/test pass on Windows is not evidence that macOS focus, Accessibility or key injection works.

Local logs include capture queue/stream-ready/first-callback durations and delivery database/main-thread-queue/paste durations. These timing records contain no audio or transcript. Compare the same device, model and cold/warm state when investigating latency; the first-callback timer starts around stream construction, while stream-ready is measured from the start request.

When a test needs audio, keep the fixture under the repository's test fixture directory. User recordings and diagnostic output belong in runtime directories and must not be added to the repository.
