# Engine Benchmarks

The Criterion suites measure throughput and overhead of core data-path operations without a GPU, model files or external services. A separate explicit speech benchmark supplies the model catalog's comparative speed references.

## Model reference measurements

The model catalog's speed hints use a separate explicit speech benchmark, not Criterion throughput. Prepare the frontend and verified native libraries as described in [Testing](testing.md), then run from `desktop/src-tauri`:

```bash
cargo run --release --locked --example model_benchmark -- tiny base small gigaam-v3 zipformer-ru-streaming
```

Pass model IDs explicitly: this downloads the selected verified weights into `desktop/src-tauri/target/model-benchmark/models` and writes results to `target/model-benchmark/references.json`. It does not open the application or use live settings, models, recordings or history. Run on an otherwise idle machine; retain reviewed results in `src/model_reference.json` with their exact artifact revisions and method version. CPU and GPU results must not be silently treated as equivalent.

Use `--language en` or `--language ru` to compare models on the same speech fixture, for example `cargo run --release --locked --example model_benchmark -- --language en parakeet-streaming-en nemotron-streaming`. Every selected model must support that language; invalid arguments fail before downloads. The run still measures both explicit language and automatic detection. Each invocation replaces `references.json`, so copy it before the next comparison. Keep results from different machines or fixture languages separate; the application's reference lookup does not distinguish the fixture language of an `auto` row.

The initial method uses pinned, SHA-256-verified public Russian speech from GigaAM's example and English JFK speech from whisper.cpp. Sources and revisions are in [model_benchmark.rs](../desktop/src-tauri/examples/model_benchmark.rs). A model is tested on Russian when supported, otherwise English, with explicit language and automatic language detection as separate cases. Each case has one warm-up and five timed runs. The output preserves median/min/max processing time, audio duration, load time, thread count, reference CPU/OS and exact model revision. Whisper uses greedy decoding with one candidate; both engines use at most eight CPU threads, matching the application. The streaming case measures the complete final pass, not live-preview latency.

RTF is processing seconds divided by audio seconds. The continuous display scale is `1 / (1 + RTF)`: real-time processing fills half the bar and processing four times faster than speech fills 80%. Higher means faster, without claiming an accuracy percentage or a percentile across computers. Personal observations use the same scale after five matching runs. Shorter than three seconds, empty output, failed or cancelled inference do not contribute. Personal groups distinguish 3–10 seconds, 10–30 seconds and longer audio, cold/warm state, language, custom-vocabulary fingerprint, model revision, hardware profile, method and requested compute context. A greater than threefold interquartile RTF spread falls back to an available reference, with an explicit explanation; without a reference the scale stays unknown.

This is an initial relative baseline using one speech clip per language, not broad accuracy, peak-memory or latency calibration. The included references do not establish GPU or macOS performance; RAM requirements remain catalog estimates. An unmeasured language or compute context borrows the same model's nearest measurement — automatic language detection, or the processor — and the application marks the bar and says so in its details; a model with no reference at all stays unknown until local dictations produce one. Updating the engine, sampling parameters or corpus requires reviewing the method version and regenerating comparable references.

The checked-in set was measured on Windows with an AMD Ryzen 7 5800X and eight CPU threads. Whisper tiny/base/small, GigaAM v3 and streaming Zipformer small use Russian speech. Parakeet unified streaming and TDT v2 use English; Nemotron streaming and Parakeet TDT v3 have both English and Russian measurements, with the Russian fixture supplying their single automatic-language reference. Turbo and the remaining catalog entries have no reference in this set; they can still acquire personal measurements through normal use. Large Whisper CPU reference runs can take substantially longer than the audio, including the repeated warm-up and timed runs.

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
cargo bench --package sotto -- wav_encoding

# Compile-only check
cargo bench --no-run --package sotto
```

Results are printed to stdout. Criterion also writes an HTML report to `target/criterion/<benchmark-group>/report/index.html`.

## What each benchmark measures

### `wav_encoding_throughput`

**What it measures:** throughput of `cloud_stt::audio_to_wav_bytes()` when encoding a 5-second 16 kHz mono f32 buffer (80 000 samples) to 16-bit PCM WAV.

This is the first step in the cloud STT path: every cloud transcription call encodes the raw audio into a WAV byte buffer before building the multipart request.

**Why it matters:** The WAV encoder is on the critical path of every cloud transcription.

If it becomes a bottleneck (e.g. after changes to the clamping or PCM conversion logic), cloud STT latency increases. It is also called once per recording, so it must stay fast at the 5-second scale.

**Budget:**

| Metric          | Threshold  | Unit       |
|-----------------|------------|------------|
| Throughput      | > 100      | MB/s       |
| Throughput      | ≫ 80 000   | samples/s  |
| Absolute time   | < 1        | ms         |

A regression below 100 MB/s for the 5-second buffer warrants investigation.

### `multipart_body_construction`

**What it measures:** wall-clock time of `cloud_stt::build_multipart_body()` for a 5-second WAV buffer, tested in two configurations:

- **without_language** — typical case when language is set to "auto".
- **with_language** — a language (e.g. `"ru"`) is pinned.

**Why it matters:** The multipart body builder runs on every cloud STT request. It concatenates the model name, optional language, and the WAV bytes into a `multipart/form-data` body.

This is CPU-cheap by design, but a regression here (e.g. from an accidental copy of the WAV bytes) could double the memory and CPU cost.

**Budget:**

| Configuration   | Threshold | Unit |
|-----------------|-----------|------|
| without_language| < 1       | ms   |
| with_language   | < 1       | ms   |

### `inference_result_clone`

**What it measures:** the cost of cloning a `whisper::InferenceResult` struct — specifically one with a realistic-length Russian transcription text (~300 characters), a `language` field, and a `session_id` / `inference_time_ms`.

**Why it matters:** `InferenceResult` is the canonical type returned by the engine to the dispatcher.

If the dispatcher (or any intermediate layer) clones it unnecessarily, the overhead scales with text length. This benchmark serves as a regression test: a sudden increase in clone cost signals an accidental clone in the hot path.

**Budget:**

| Metric        | Threshold | Unit  |
|---------------|-----------|-------|
| Clone latency | < 5       | µs    |

If clone time exceeds 5 µs for realistic text, profile the dispatcher for unnecessary clones.

### `mutex_recover_lock_overhead`

**What it measures:** overhead of `mutex_recover::lock()` (which recovers from a poisoned mutex) compared to a plain `Mutex::lock().unwrap()` on a **healthy** (non-poisoned) mutex. Both operations are measured back-to-back on the same `Mutex<u64>`.

**Why it matters:** The `mutex_recover` helper is used throughout the dispatcher for FSM, registry, and cancel-flag state.

On the happy path (99.9%+ of all lock acquisitions), the lock must not add measurable overhead compared to a normal `lock().unwrap()`. If it does, the dispatcher's lock contention profile changes and overall recording latency may increase.

**Budget:**

| Comparison              | Threshold |
|-------------------------|-----------|
| `mutex_recover::lock`   | ≤ 1.10 ×  `Mutex::lock().unwrap()` latency |
| Absolute latency        | ≤ 100     ns |

A relative overhead above 10% or an absolute latency above 100 ns warrants a review of the `mutex_recover::lock()` implementation.

## Regression budgets summary

| Benchmark                     | Key metric              | Budget                  |
|-------------------------------|-------------------------|-------------------------|
| `wav_encoding_throughput`     | Throughput (MB/s)       | > 100 MB/s for 5s audio |
| `multipart_body_construction` | Wall time (ms)          | < 1 ms                  |
| `inference_result_clone`      | Clone latency (µs)      | < 5 µs                  |
| `mutex_recover_lock_overhead` | Relative overhead       | ≤ 10% vs `std::sync::Mutex` |

## Artifacts and CI comparison

Criterion saves detailed measurement data under `desktop/src-tauri/target/criterion/<benchmark-group>/`. Each run produces:

- `new/raw.csv` — raw measurements (nanoseconds per iteration)
- `new/estimates.json` — summary statistics (mean, std dev, slope, etc.)
- `report/index.html` — browsable HTML report with violin plots

To compare against a saved baseline:

```bash
# Save a baseline from the current codebase (e.g. on `main`):
cargo bench --package sotto -- --save-baseline main

# After a change, compare:
cargo bench --package sotto -- --baseline main
```

For CI comparison, archive the `target/criterion` directory as a build artifact in the CI pipeline. The full raw data can be retrieved from:

```yaml
- name: Upload benchmark artifacts
  uses: actions/upload-artifact@v4
  with:
    name: criterion-reports
    path: desktop/src-tauri/target/criterion/
```

Run benchmarks on a local machine or dedicated runner with a consistent CPU. CI should **not** run the benchmarks, but it can archive a baseline from those runs.

CI checks benchmark compilation through `cargo clippy --all-targets`, which runs in the `build-test` job on every target OS. A separate benchmark build was removed because it took 16 of that lint pass's 23 minutes.

## Adding new benchmarks

1. Add a new `fn my_bench(c: &mut Criterion)` in `desktop/src-tauri/benches/engine_bench.rs`.
2. Register it in the `criterion_group!` macro's target list.
3. Document the benchmark and its budget in this file.
4. Run `cargo clippy --all-targets` to confirm it compiles — that is the check CI runs.
