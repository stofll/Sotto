# Testing

Run the frontend checks from the repository root:

```bash
cd desktop
pnpm install --frozen-lockfile
pnpm exec tsc --noEmit
pnpm exec tsc --noEmit -p ../tests/ui/tsconfig.json
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

CI runs formatting once on Linux and Clippy on Linux, Windows, and macOS so platform-specific code is also linted. The CI workflow is authoritative for the operating-system matrix. Run relevant checks locally before a pull request; CI repeats them on clean runners. Local success does not replace CI, and CI does not replace native verification.

## Browser UI tests

Run the Python/Playwright suite for UI changes. [Browser UI testing](ui-testing.md) documents setup, focused commands, test boundaries and failure artifacts; `.github/workflows/ui-tests.yml` runs Chromium and WebKit on pull requests. Keep the existing frontend checks above: browser tests complement their logic, IPC-contract, i18n and bundle checks.

## Release automation

Run `node --test scripts/release-version.test.mjs scripts/check-release-source.test.mjs` from the repository root when changing version preparation. These tests use temporary Git repositories and metadata copies; they do not bump the working copy, push tags, or launch the application. PR CI also runs them and checks version consistency with `sh scripts/check-version.sh`.

On Windows, run these release tests from Git Bash or put Git for Windows' `bin` directory on `PATH`: the tests invoke `sh` to check version consistency.

Validate workflow edits with `actionlint`. A local pass cannot verify GitHub repository permissions, the release App’s bypass permission, or signed artifact publication; those require a real Prepare Release run after the workflow is merged.

For Windows packaging changes, run `node --test scripts/build-portable.test.mjs scripts/build-windows-installer.test.mjs` on Windows. These use temporary fixtures to verify ZIP cleanup and native preparation order, cache recovery and failure handling; they do not build or install a signed application. Run `node --test scripts/generate-native-inventory.test.mjs` when changing native dependency inventory or its locks.

## Prepared speech and real models

Every PR runs deterministic tests without speech downloads, plus an explicitly selected real Whisper CPU smoke test on Windows, macOS and Linux. Windows and macOS also run the Sherpa smoke test. The ignored marker keeps network/model downloads out of an ordinary local `cargo test`; the CI commands explicitly include those tests.

Whisper uses the repository's portable [CPU instruction baseline](RELEASE.md#cpu-instruction-baseline), including in GPU builds. CI cache keys include its configuration. When adopting or changing that baseline in an existing local build directory, run `cargo clean -p whisper-rs-sys` before rebuilding (also `cargo clean --release -p whisper-rs-sys` for release builds): Cargo does not track edits inside the CMake toolchain file. From the repository root, run `node scripts/check-ggml-baseline.mjs desktop/src-tauri/target/debug` to check the compiled debug baseline, or pass the full target directory to check all compiled profiles. Test the benchmark's argument validation with `cargo test --locked --example model_benchmark` from `desktop/src-tauri`.

Reproduce the model tests locally from `desktop/src-tauri` when changing capture, resampling, model loading or inference:

```bash
cargo test --locked --test test_whisper_engine -- --ignored --nocapture
# Windows and macOS:
cargo test --locked --test test_sherpa_runtime -- --ignored --nocapture
```

After changing the Parakeet unified export, run `cargo test --locked --test test_parakeet_streaming -- --ignored --nocapture`. This separate test downloads approximately 632 MB into a temporary directory, or copies weights from `SOTTO_TEST_PARAKEET_DIR` when supplied; both paths verify the catalog hashes. It checks repeated full-file recognition, chunked input, final words and reset after cancellation on public English speech.

These tests download verified weights into isolated temporary directories. Their public speech fixtures are pinned by source revision and SHA-256 in the test sources. Whisper tiny checks two recognizable English phrases from the upstream JFK sample and repeated inference with the same state. Sherpa checks loading, silence, reset/reload and the final word of a Russian streaming sample. These are inference regressions, not a broad accuracy benchmark or the full hotkey-to-paste flow.

## Formatter corpus tests

`cargo test --locked --test formatter_corpus` runs the reviewed synthetic examples in [the formatter fixture](../desktop/src-tauri/tests/fixtures/formatter/cases.json). Each case declares its input, settings, expected output and category. The test checks the full formatter, preview, dictation/file delivery and repeated processing. A separate delivery expectation covers intentional differences such as restoring raw text after incidental empty cleanup; a case may explicitly opt out of repeated-processing checks when its configured replacement produces a result that later cleanup can change. This corpus also runs in the ordinary Cargo test suite.

For a correction change, add both an example that should be corrected and a similar valid or ambiguous example that must be preserved. Include interaction cases for replacement modes, technical text and language settings rather than testing only one step. Review expected outputs as product behavior; do not regenerate them from the implementation. Ordinary vocabulary, term dictionaries, replacement rules and deletion rules have different purposes and need separate categories, even though they share the same pipeline.

Two ignored tests run the formatter over a dump of real transcriptions instead of hand-written cases: `collapse_over_corpus` reports how often the filler cleanup rewrites a line, and `dictionary_over_corpus` reports what a term list does to the same text. Both need a corpus file with one transcription per line, supplied through `SOTTO_CORPUS`, and the dictionary test also needs a term list through `SOTTO_DICT`, one term per line. They print the changed lines and a summary, and assert nothing: the result is a stability rate to read, not a pass/fail threshold, which is why they stay out of ordinary runs.

```bash
cd desktop/src-tauri
SOTTO_CORPUS=corpus.txt cargo test --locked --lib formatter::tests::collapse_over_corpus -- --ignored --nocapture
SOTTO_CORPUS=corpus.txt SOTTO_DICT=dict.txt cargo test --locked --lib formatter::custom_words_tests::dictionary_over_corpus -- --ignored --nocapture
```

Keep a user's transcription dump out of the repository: place the corpus outside the working tree, and if a case has to be committed, reduce it to a synthetic fixture.

To compare the complete local pipeline, run `cargo test --locked --lib formatter::tests::format_over_corpus -- --ignored --nocapture` with `SOTTO_CORPUS`, `SOTTO_FORMAT_CONFIG` (a JSON file containing only language and formatting settings), and `SOTTO_FORMATTED` (a new output path). The test writes one JSON string per input line, reports first-pass, median and maximum elapsed times, and refuses to overwrite an existing output file. Keep all three files outside the repository and review the changes against the input; timings and changed-line counts alone do not establish correction accuracy.

When expanding accuracy coverage, use a reviewed corpus with expected transcripts and explicit word-error tolerances per model/language. Include silence, short speech, noise and trailing speech. Keep this separate from deterministic timeout, cancellation and lifecycle tests so a recognition-quality change cannot hide a broken session transition.

## Native verification before release

On a Windows desktop session, run `cargo test --locked --lib language_changes_preserve_one_native_tray -- --ignored --nocapture` from `desktop/src-tauri` after native runtime preparation. It creates a tray in a minimal Tauri application, applies repeated RU/EN configuration changes, and checks that the original native tray resource survives without duplicates. It creates no application windows, loads no application plugins, and never opens configuration, history, recordings or models. This opt-in test requires a native desktop; browser mocks cannot verify icon ownership. Separately check the translated menu labels, left-click popup and right-click actions in a packaged application on each affected OS.

Run `cargo test --locked --lib native_hotkey_rebinding_preserves_ownership -- --ignored --nocapture` separately to check Windows shortcut ownership. This isolated app loads only the shortcut plugin, temporarily registers Ctrl+Alt+Shift+F23/F24, checks aliases, replacement, invalid settings and reacquisition, and releases its own shortcuts on exit. It sends no key presses and opens no microphone or application data. A conflicting external shortcut fails the test instead of unregistering another application's binding. Use separate Cargo invocations for these native tests because Tauri permits only one event loop per process; neither test verifies real hotkey-to-paste delivery.

Changes to microphone capture, global shortcuts, clipboard behavior, installers, or model loading require checks on each affected OS with a packaged application. Establish where configuration, history, recordings and models will be stored before launching it, and isolate test data from the developer's live data. Never reset live data to prepare a test.

For capture or audio-dependency changes, also run the native recorder regression on each affected OS with an available microphone and microphone permission:

```bash
cd desktop/src-tauri
cargo test --locked --lib native_microphone_repeated_sessions_preserve_duration -- --ignored --nocapture
```

This records five short sessions with the system default microphone, then five with the same device explicitly selected through enumeration. It checks PCM duration against elapsed time and verifies that callbacks release their buffers after stop. Audio stays in memory; the test does not load app configuration, save recordings, transcribe, or write history.

It is opt-in because ordinary CI runners do not guarantee a microphone or permission; a skipped hardware test is not evidence of correct capture. Synthetic PCM and prepared-speech inference tests cannot verify native stream teardown.

- Start/stop through the hotkey, in toggle and push-to-talk modes. Repeat immediately after silence, cancellation and a recoverable failure.
- Cancel during capture, inference and optional LLM processing. Verify that cancelled text is neither inserted nor written as a successful history entry.
- Test 44.1 kHz and 48 kHz input where available, microphone disconnect and device-open failure. Verify the next recording still works.
- Verify paste in a native text editor and a browser input, with Latin and Cyrillic layouts. Test held/released hotkey modifiers, focus changes, clipboard-only mode and manual paste after an automatic-paste failure. On macOS this requires Accessibility access for the actual installed build.
- Transcribe a file while an editable field is focused: no automatic paste or dictation history entry. Competing microphone/file jobs must be refused without interrupting the active job.
- Exercise local Whisper and supported Sherpa models, configured cloud STT and optional LLM formatting. Verify timeout/error fallback with a controlled endpoint. Check packaged GPU inference on the applicable release target.

Record the OS, application build, cases actually run and limitations. A compile/test pass on Windows is not evidence that macOS focus, Accessibility or key injection works.

Local logs include capture queue/stream-ready/first-callback durations and delivery database/main-thread-queue/paste durations. These timing records contain no audio or transcript. Compare the same device, model and cold/warm state when investigating latency; the first-callback timer starts around stream construction, while stream-ready is measured from the start request.

When a test needs audio, keep the fixture under the repository's test fixture directory. User recordings and diagnostic output belong in runtime directories and must not be added to the repository.
