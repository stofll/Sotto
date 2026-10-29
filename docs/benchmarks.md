# Engine Benchmarks

The Criterion suite measures the production cloud upload encoder and selected primitive costs without a GPU, model files or external services. A separate explicit speech benchmark supplies the model catalog's comparative speed references. Neither suite measures desktop startup, native capture, stop-to-paste latency or model memory reclamation.

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

Historical results keep their original method labels; incompatible rows remain available for review but appear as unmeasured in the application until a new run is reviewed. Speed comes only from this bundled reference; dictation performance is not recorded.

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

## Adding new benchmarks

1. Add a new `fn my_bench(c: &mut Criterion)` in `desktop/src-tauri/benches/engine_bench.rs`.
2. Register it in the `criterion_group!` macro's target list.
3. Document the benchmark and its budget in this file.
4. Run `cargo clippy --all-targets` to confirm it compiles — that is the check CI runs.

## User dictionary scaling

Run `cargo test --locked --lib formatter::custom_words_tests::dictionary_scaling_benchmark -- --ignored --nocapture` after native runtime preparation to compare 100, 1,000 and 10,000 synthetic terms. The test reports the median of five construction and correction runs over six similar technical tokens, including longer terms; it does not load user dictionaries or assert a machine-dependent timing threshold.

The 2026-09-29 Windows debug measurement reported construction/correction times of 0.652/1.857 ms for 100 terms, 6.347/16.911 ms for 1,000 and 64.960/135.218 ms for 10,000. These are local debug timings, not release latency targets. Grouping candidates by length did not improve this fixture and was not retained.

New dictionary edits now enforce the [documented term limits](dictionaries.md#spelling-and-limits). Existing oversized dictionaries remain intact and can still be reduced; their processing cost is not silently capped or skipped.
