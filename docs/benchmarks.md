# Engine Benchmarks

The Criterion suite measures the production cloud upload encoder, live text formatting, history queries, capture sample conversion and VAD preparation without a GPU, model files or external services. A separate explicit speech benchmark supplies the model catalog's comparative speed references. These component measurements do not establish desktop startup, native callback scheduling, stop-to-paste latency or model memory reclamation.

## Model reference measurements

The model catalog's speed hints use a separate explicit speech benchmark, not Criterion throughput. Prepare the frontend and verified native libraries as described in [Testing](testing.md), then run from `desktop/src-tauri`:

```bash
cargo run --release --locked --example model_benchmark -- --output target/model-benchmark/cpu-ru.json tiny base small gigaam-v3 zipformer-ru-streaming
```

Pass model IDs explicitly: this downloads the selected verified weights into `desktop/src-tauri/target/model-benchmark/models` and writes results to the explicit `--output` path (default: `target/model-benchmark/references.json`). A fresh run refuses to overwrite an existing file. It does not open the application or use live settings, models, recordings or history. Run on an otherwise idle machine; retain reviewed results in `src/model_reference.json` with their exact artifact revisions and method version. CPU and GPU results must not be silently treated as equivalent.

Use `--language en` or `--language ru` to compare models on the same speech fixture, for example `cargo run --release --locked --example model_benchmark -- --language en parakeet-streaming-en nemotron-streaming`. Every selected model must support that language; invalid arguments fail before downloads. The run still measures both explicit language and automatic detection. Use a separate `--output` file for each language and machine. Add `--resume` to reopen a result file and skip completed model/language cases; a partial case is repeated from its warm-up.

Schema 2 resume rejects older output formats, incomplete records, changed fixtures, and a different hardware, method, source, executable or lockfile context. Catalog mode reuses the evaluation runner's Git revision, source, executable and dependency fingerprints; rebuilding or changing source requires a new output file. Keep results from different machines or fixture languages separate; the application's reference lookup does not distinguish the fixture language of an `auto` row.

For a small pilot using only weights already present in the isolated benchmark cache, run:

```bash
cargo run --release --locked --example model_benchmark -- --no-download --output target/model-benchmark/pilot-cpu-ru.json --language ru tiny gigaam-v3
# Continue the same run after interruption, or verify that completed cases are skipped:
cargo run --release --locked --example model_benchmark -- --no-download --resume --output target/model-benchmark/pilot-cpu-ru.json --language ru tiny gigaam-v3
```

`--no-download` prevents model-weight downloads; the selected public speech fixtures are still fetched and checksum-verified. Model files are verified before use. Each language case is checkpointed through an atomic file replacement, so stopping a later model preserves earlier results. Progress reports each warm-up and timed run. Results retain the five individual durations, median/min/max, load time and its scope, RTF and its inverse RTFx, measurement timestamp, fixture hashes and hardware/build fingerprints. Keep generated run files outside version control; only reviewed catalog references belong in `src/model_reference.json`.

The catalog reference mode remains CPU-only and uses one natural speech clip per language. Its `load_ms` starts after weight verification and includes native context creation and Whisper state allocation. Verification has already read the weights, so this is not a cold-cache measurement.

The application initializes Whisper state at first transcription instead; this value is not its model-ready or first-dictation latency. The five timed samples measure inference, while model load is sampled once per model.

The separate [dataset evaluation mode](speech-evaluation.md) measures prepared corpora and recognition errors with explicit development/holdout selection; keep those results separate from catalog speed references. Neither mode verifies GPU execution or live-preview latency.

Whisper references require an optimized whisper.cpp build. With its default Visual Studio generator, cmake-rs replaces the Release compiler flags and drops `/O2`, which made Windows builds of whisper.cpp about ten times slower; [ggml-baseline.cmake](../scripts/ggml-baseline.cmake) restores the flags and `scripts/check-ggml-baseline.mjs` rejects a build cache without them. References taken before method `sotto-stt-v2` measured that unoptimized build and are not comparable.

The initial method uses pinned, SHA-256-verified public Russian speech from GigaAM's example and English JFK speech from whisper.cpp. Sources and revisions are in [model_benchmark.rs](../desktop/src-tauri/examples/model_benchmark.rs). A model is tested on Russian when supported, otherwise English, with explicit language and automatic language detection as separate cases. Each case has one warm-up and five timed runs. The output preserves median/min/max processing time, audio duration, load time, thread count, reference CPU/OS and exact model revision. Whisper uses greedy decoding with one candidate and, like the application, skips language detection for English-only models; both engines use at most eight CPU threads, matching the application. The streaming case measures the complete final pass, not live-preview latency.

RTF is processing seconds divided by audio seconds. Cards classify RTF ≤ 0.25 as High, RTF ≤ 1 as Medium, and larger values as Low, using the internal score `1 / (1 + RTF)`. These thresholds describe throughput, not accuracy or a percentile across computers.

Cards use a compatible bundled CPU reference for the language selected in Settings, falling back to the automatic-detection row, and display it at one third, two thirds or full fill.

The build derives engine versions from `Cargo.lock` and checks Sherpa against `scripts/sherpa-runtime.lock`. A reference must match its own engine: upgrading Sherpa invalidates Sherpa speed hints without invalidating unchanged Whisper measurements.

Historical results keep their original method labels; incompatible rows remain available for review but appear as unmeasured in the application until a new run is reviewed. Live dictation timings do not update the bundled catalog reference.

Pick the row to retain accordingly: multilingual Whisper's `auto` rows run 1.5–2x its explicit-language rows because of the detection pass, while the Sherpa and English-only Whisper rows differ by at most a few percent. The fixture language itself — Russian versus English speech of comparable length — moves RTF by 2–13%, so it is a note on the measurement rather than a second scale.

The historical reviewed catalog includes 25 built-in models measured on Windows with a Ryzen 7 5800X and eight CPU threads on 2026-10-09: 50 explicit-language/automatic cases, plus English-language references for Nemotron and Parakeet TDT v3. The run was repeated after an interrupted one under background load, and the retained run agrees with the earlier Sherpa references. Qwen3 (RTF about 0.29) and Whisper turbo with an explicit language (RTF about 1.0) sit at level boundaries, so small run-to-run variation can move them by one level. Each new case preserves five raw times and the source fixture language. Different speech fixtures and CPU/GPU contexts are not interchangeable; the application never mixes them, and the interface does not repeat the provenance on every card.

These measurements do not assess transcription accuracy or predict performance on other machines. The stored Sherpa rows describe 1.13.7; builds using 1.13.8 do not reuse them as current speed hints. Large Whisper CPU runs can take longer than the audio, so a full catalog run takes about half an hour.

The Windows comparison for [#19](https://github.com/stofll/Sotto/issues/19) reproduced slow Parakeet unified streaming with the previous 560 ms export outside Sotto: the official sherpa-onnx 1.13.7 CLI takes about 24 s for the 11 s English fixture, versus 23.6 s through Sotto. Nemotron takes about 2.3 s and 2.15 s respectively; offline Parakeet TDT finishes in about 0.55 s through Sotto. The comparison uses the same model files, matching ONNX Runtime DLLs, eight CPU threads, greedy decoding, disabled endpointing, no left padding and 0.6 s right padding. Sotto reports the median of five warm runs; the official CLI was launched three times with model loading excluded from its reported decoding time, which it rounds to two significant digits. These are throughput measurements, not a native live-preview latency or recognition-quality test.

The previous 560 ms Parakeet unified encoder declares 560 left, 16 center and 40 right feature frames. Upstream [buffered decoding](https://github.com/k2-fsa/sherpa-onnx/blob/917bed95c8e5c7c18aa4d69fea42e9ef8ef0a60e/sherpa-onnx/csrc/online-recognizer-transducer-nemo-parakeet-unified-impl.h) reruns the encoder on that overlapping 6.16 s window for each 0.16 s step; Nemotron carries encoder state between chunks. Thus equal model sizes and nominal latency presets do not imply comparable computation. This explains why the integration can be identical while throughput differs substantially. Reducing CPU threads from eight to four, two or one did not bring this export below real time. The official 1120 ms export completed the same fixture in about 7.6 s with eight threads, and is now the catalog variant. The updated Sotto reference measures 7.25 s with English selected and 7.38 s with automatic language selection on the same 11 s fixture (five warm runs per setting). It increases the nominal streaming latency; macOS performance and quality on a broader speech corpus remain unverified. The catalog includes measured references for the pinned variants; changing an export requires new references and separate latency and quality checks.

## How to run Criterion

Run these commands from `desktop/src-tauri`:

```bash
# All benchmarks
cargo bench --package sotto

# Just this suite
cargo bench --package sotto --bench engine_bench

# Filtered by name (criterion supports substring matching)
cargo bench --package sotto --bench engine_bench -- streaming_upload

# Component suites can be run separately to keep a comparison bounded:
cargo bench --locked --package sotto --bench engine_bench -- live_formatter
cargo bench --locked --package sotto --bench engine_bench -- history_page
cargo bench --locked --package sotto --bench engine_bench -- capture_samples
cargo bench --locked --package sotto --bench engine_bench -- vad_prepare

# Compile-only check
cargo bench --no-run --package sotto
```

Results are printed to stdout. With the current `criterion` features, JSON measurements are also saved under `target/criterion/<benchmark-group>/new/`; HTML and CSV reports are disabled.

## What each benchmark measures

### `streaming_upload`

This measures the encoder used by `cloud_stt::transcribe`: multipart fields, WAV header, and PCM16 conversion in chunks of at most 32,768 samples. Each iteration consumes and drops chunks without collecting the complete body, contacting a provider or timing a network connection. Audio input is prepared before measurement.

Cases cover 5, 60 and 600 seconds of synthetic 16 kHz mono audio, each with automatic or explicit Russian language selection. Criterion reports complete encoded body bytes per second, including the WAV and multipart headers, using one throughput unit per case. Compare each duration and language case against its own baseline.

The former `wav_encoding` and `multipart_body` groups exercised full-buffer compatibility helpers that the production uploader no longer calls. Their historical results are not a baseline for the streaming group. Wire-equivalence tests compare the production iterator with those helpers across empty audio and chunk boundaries and verify chunk-size bounds.

Establish a release benchmark baseline on a known machine before setting a streaming-encoder latency budget. A byte-size bound is not a measurement of whole-process peak memory: request buffering, the retained PCM input and the native application must be profiled separately.

### `live_formatter`

Every iteration calls the same `format_transcription_with_config_value` API as dictation and file delivery. The deterministic Russian text, personal vocabulary and replacement rules are in `benches/support/formatting.rs`. Cases cover default settings, 0/100/1,000 terms with 50 replacement rules, cleanup switched off with the largest settings (terms and rules still apply), alternating unrelated settings, and alternating replacement text that must rebuild the cached pipeline.

Settings and input are prepared outside measurement. Warm repeated calls include typed configuration parsing, cache lookup and text processing; initial spelling-dictionary initialization is excluded. Compare a fresh process's first call separately, especially when checking disabled formatting. The relevant-setting case measures a cache miss on each alternation; the unrelated-setting case should reuse the same prepared matchers. Keep output equality and the reviewed formatter corpus alongside any timing comparison.

### `history_page`

The benchmark imports the production storage query module and measures a query plus JSON serialization of its result. A synthetic in-memory SQLite database contains 1,000, 10,000 or 100,000 retained rows, repeated timestamps, all three processing statuses, three transcript stages and pipeline JSON. One percent of rows contain a Russian search marker. The fixture uses the production history schema migrations and never opens application data.

Cases request the first page, last page and a Unicode substring search, with 50 rows per page and unlimited retention for scaling. The 100,000-row case is an explicit stress case; the default retention cap is 1,000. Database construction is outside measurement. Counts and filtering still examine the retained dataset, and late offset pages can cost more than the first page. Bounded output does not imply constant query time.

When comparing against the earlier full-history API, report returned rows and serialized bytes as well as query and serialization time. The earlier API sends the complete retained list before frontend filtering. This benchmark excludes IPC transport, recording-file checks, React rendering and filesystem I/O; measure those separately in the application.

### `capture_samples`

These cases call the production sample-conversion and stereo-to-mono helpers on 960 interleaved I16 or F32 samples (480 stereo frames). Scratch capacity is warmed before timing, matching repeated callbacks of the same size. They exclude CPAL scheduling, RMS calculation, resampling, recording-buffer locking and inference queues. A separate allocation counter is needed to establish allocation counts; elapsed time alone cannot establish that a callback allocates nothing.

### `vad_prepare` and `vad_cancelled_before_prepare`

Preparation uses the production VAD on 60 and 600 seconds of deterministic synthetic 16 kHz audio with trimming enabled. The active cases include the detector pass and any trim decision. Cancellation checks must preserve the same samples, offsets and speech timing when the flag stays clear.

The cancelled case starts with the cancellation flag set and measures the early return, so it deliberately has no audio-throughput statistic. It verifies avoided work before recognition, not the delay from a real keypress or cancellation halfway through an operating-system callback. Synthetic audio timing does not evaluate speech-detection accuracy.

### `inference_result_clone`

This measures one clone of a realistic `InferenceResult`, including text, language and model ID. The existing investigation threshold is 5 µs per clone. It measures allocation/copy cost only; adding another clone in the dispatcher does not change this benchmark's workload and requires an integration trace or allocation profile.

### `mutex_recover_lock_overhead`

This compares `mutex_recover::lock()` with `Mutex::lock().unwrap()` on a healthy uncontended mutex. Existing investigation thresholds are 10% relative overhead or 100 ns absolute time. Neither case measures lock contention, engine queue delay or long critical sections.

These thresholds are manual review guides, not executable CI failure budgets. A stable primitive benchmark does not establish application responsiveness.

## Artifacts and CI comparison

Criterion saves detailed measurement data under `desktop/src-tauri/target/criterion/<benchmark-group>/`. The enabled features produce JSON files such as `new/sample.json` (sampling data) and `new/estimates.json` (summary statistics). The `html_reports` and `csv_output` features are not enabled, so do not expect `report/index.html` or `new/raw.csv`.

To compare against a saved baseline:

```bash
# Save a baseline from the current codebase (e.g. on `main`):
cargo bench --package sotto --bench engine_bench -- --save-baseline main

# After a change, compare:
cargo bench --package sotto --bench engine_bench -- --baseline main
```

Keep the complete `target/criterion` directory with the measured revision and hardware/build context when retaining a baseline. The ordinary workflow does not run or upload Criterion measurements. Use an otherwise idle local machine or a dedicated runner with a consistent CPU for performance comparisons; shared hosted-runner timings are not a reliable small-regression gate.

CI checks benchmark compilation through `cargo clippy --all-targets`, which runs in the `build-test` job on every target OS. A separate benchmark build was removed because it took 16 of that lint pass's 23 minutes.

## Reproducible before/after comparisons

Build both revisions with the pinned toolchain and the same optimization profile, target architecture and fixture definitions. Record the Git revisions, any uncommitted source hashes, complete lockfiles, executable hashes, CPU/OS, enabled features, thread count and whether the process and filesystem cache were warm. A benchmark introduced by the change must use the same harness and inputs on both implementations; changing the workload invalidates a percentage comparison.

Run functional equivalence checks before timing. Use several fresh baseline/candidate process pairs, reversing their order on alternating pairs, and retain individual observations. Report the median of per-process medians and their range rather than choosing the fastest run. Avoid concurrent builds or tests during collection. These controls reduce local noise but do not turn a shared virtual machine into a dedicated performance runner.

Where native prerequisites are unavailable, an isolated component harness may compile the unchanged production modules and their real dependencies. Keep the harness, exact source snapshots, lockfile and build command with its output. Explicitly label such results as component measurements and describe any omitted native wrapper. Source-import checks do not replace the application's platform builds or native UI verification.

For memory, distinguish current resident memory from process peak RSS, allocator counts and GPU memory. A process-lifetime peak cannot show whether memory was released. Keep database population and audio-fixture allocation outside the timed query or detector region, and state whether those inputs are included in the reported memory baseline.

## Native startup and model lifecycle protocol

Run this protocol on the supported Windows and macOS targets with the same already-verified model files and an isolated test profile. Set `SOTTO_DATA_DIR`, `SOTTO_MODELS_DIR` and `SOTTO_LOG_DIR` to explicit test directories; a portable build's adjacent data directory takes precedence, so verify the resolved paths before launching. Keep the release build, selected engine, CPU/GPU setting, driver and application settings identical between revisions.

| Scenario | Capture | Interpretation |
| --- | --- | --- |
| Fresh process, local mode | Launch-to-responsive-window, verification/context timings, current RSS and first-dictation latency | Separate UI readiness from model readiness and first Whisper state allocation. |
| Fresh process, cloud mode | The same launch/RSS observations and model lifecycle log | Verify that startup does not create a local model context. No provider request is needed. |
| Resident model, repeated dictation | At least five identical fixture runs, recognition time and delivery time | This is the warm path; report recognition and complete delivery separately. |
| Restore after idle unload | Request/verification/context/state timings and current RSS before/after restore | Hash verification may warm the filesystem cache before context creation. |
| Switch to cloud while idle and while busy | Time to safe unload, current RSS before unload and after 1/5/30 seconds | Active capture/file ownership must finish before the deferred release. |
| Repeated load/release | Ten bounded cycles, current RSS/private memory and GPU memory after each settle period | Distinguish retained allocator/driver memory from an increasing residency trend. |

The application logs `model timing` stages for request, verification, context creation, first-inference state and unload. A successful destructor timing proves the native object was released; it does not prove that the allocator or GPU driver returned every page immediately. Match events by the run and requested transition rather than adding overlapping timers together.

Use the catalog pilot command above for a bounded CPU model comparison with existing weights. Its `load_ms` remains a single post-verification observation per model, and it does not measure application startup, idle policy or RSS. Call a new-process run "process cold" unless the filesystem-cache state was actually controlled and recorded. Do not flush the user's system caches or substitute a warmed catalog load for a cold-start result.

If a supported native environment or verified model is unavailable, leave those observations unmeasured and state the missing prerequisite. Component benchmark improvements cannot supply those numbers.

## Adding new benchmarks

1. Add a new `fn my_bench(c: &mut Criterion)` in `desktop/src-tauri/benches/engine_bench.rs`.
2. Register it in the `criterion_group!` macro's target list.
3. Document the benchmark and its budget in this file.
4. Run `cargo clippy --all-targets` to confirm it compiles — that is the check CI runs.

## User dictionary scaling

Run `cargo test --locked --lib formatter::custom_words_tests::dictionary_scaling_benchmark -- --ignored --nocapture` after native runtime preparation to compare 100, 1,000 and 10,000 synthetic terms. The test reports the median of five construction and correction runs over six similar technical tokens, including longer terms; it does not load user dictionaries or assert a machine-dependent timing threshold.

The 2026-09-29 Windows debug measurement reported construction/correction times of 0.652/1.857 ms for 100 terms, 6.347/16.911 ms for 1,000 and 64.960/135.218 ms for 10,000. These are local debug timings, not release latency targets. Grouping candidates by length did not improve this fixture and was not retained.

New dictionary edits now enforce the [documented term limits](dictionaries.md#spelling-and-limits). Existing oversized dictionaries remain intact and can still be reduced; their processing cost is not silently capped or skipped.
