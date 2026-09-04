# Engine Benchmarks

Performance baseline for the Rust speech-to-text engine. These benchmarks
measure throughput and overhead of core data-path operations that _don't_
require a GPU, model files, or external services.

## How to run

Run these commands from `desktop/src-tauri`:

```bash
# All benchmarks
cargo bench --package whisper-desktop

# Just this suite
cargo bench --package whisper-desktop --bench engine_bench

# Filtered by name (criterion supports substring matching)
cargo bench --package whisper-desktop -- wav_encoding

# Compile-only check
cargo bench --no-run --package whisper-desktop
```

Results are printed to stdout. Criterion also writes an HTML report to
`target/criterion/<benchmark-group>/report/index.html`.

## What each benchmark measures

### `wav_encoding_throughput`

**What it measures:** throughput of `cloud_stt::audio_to_wav_bytes()` when
encoding a 5-second 16 kHz mono f32 buffer (80 000 samples) to 16-bit PCM
WAV. This is the first step in the cloud STT path: every cloud transcription
call encodes the raw audio into a WAV byte buffer before building the
multipart request.

**Why it matters:** The WAV encoder is on the critical path of every cloud
transcription. If it becomes a bottleneck (e.g. after changes to the clamping
or PCM conversion logic), cloud STT latency increases. It is also called
once per recording, so it must stay fast at the 5-second scale.

**Budget:**

| Metric          | Threshold  | Unit       |
|-----------------|------------|------------|
| Throughput      | > 100      | MB/s       |
| Throughput      | ≫ 80 000   | samples/s  |
| Absolute time   | < 1        | ms         |

A regression below 100 MB/s for the 5-second buffer warrants investigation.

### `multipart_body_construction`

**What it measures:** wall-clock time of `cloud_stt::build_multipart_body()`
for a 5-second WAV buffer, tested in two configurations:

- **without_language** — typical case when language is set to "auto".
- **with_language** — a language (e.g. `"ru"`) is pinned.

**Why it matters:** The multipart body builder runs on every cloud STT
request. It concatenates the model name, optional language, and the WAV
bytes into a `multipart/form-data` body. This is CPU-cheap by design, but
a regression here (e.g. from an accidental copy of the WAV bytes) could
double the memory and CPU cost.

**Budget:**

| Configuration   | Threshold | Unit |
|-----------------|-----------|------|
| without_language| < 1       | ms   |
| with_language   | < 1       | ms   |

### `inference_result_clone`

**What it measures:** the cost of cloning a `whisper::InferenceResult`
struct — specifically one with a realistic-length Russian transcription text
(~300 characters), a `language` field, and a `session_id` / `inference_time_ms`.

**Why it matters:** `InferenceResult` is the canonical type returned by the
engine to the dispatcher. If the dispatcher (or any intermediate layer)
clones it unnecessarily, the overhead scales with text length. This benchmark
serves as a regression test: a sudden increase in clone cost signals an
accidental clone in the hot path.

**Budget:**

| Metric        | Threshold | Unit  |
|---------------|-----------|-------|
| Clone latency | < 5       | µs    |

If clone time exceeds 5 µs for realistic text, profile the dispatcher for
unnecessary clones.

### `mutex_recover_lock_overhead`

**What it measures:** overhead of `mutex_recover::lock()` (which recovers
from a poisoned mutex) compared to a plain `Mutex::lock().unwrap()` on a
**healthy** (non-poisoned) mutex. Both operations are measured back-to-back
on the same `Mutex<u64>`.

**Why it matters:** The `mutex_recover` helper is used throughout the
dispatcher for FSM, registry, and cancel-flag state. On the happy path
(99.9%+ of all lock acquisitions), the lock must not add measurable overhead
compared to a normal `lock().unwrap()`. If it does, the dispatcher's lock
contention profile changes and overall recording latency may increase.

**Budget:**

| Comparison              | Threshold |
|-------------------------|-----------|
| `mutex_recover::lock`   | ≤ 1.10 ×  `Mutex::lock().unwrap()` latency |
| Absolute latency        | ≤ 100     ns |

A relative overhead above 10% or an absolute latency above 100 ns warrants
a review of the `mutex_recover::lock()` implementation.

## Regression budgets summary

| Benchmark                     | Key metric              | Budget                  |
|-------------------------------|-------------------------|-------------------------|
| `wav_encoding_throughput`     | Throughput (MB/s)       | > 100 MB/s for 5s audio |
| `multipart_body_construction` | Wall time (ms)          | < 1 ms                  |
| `inference_result_clone`      | Clone latency (µs)      | < 5 µs                  |
| `mutex_recover_lock_overhead` | Relative overhead       | ≤ 10% vs `std::sync::Mutex` |

## Artifacts and CI comparison

Criterion saves detailed measurement data under
`desktop/src-tauri/target/criterion/<benchmark-group>/`. Each run produces:

- `new/raw.csv` — raw measurements (nanoseconds per iteration)
- `new/estimates.json` — summary statistics (mean, std dev, slope, etc.)
- `report/index.html` — browsable HTML report with violin plots

To compare against a saved baseline:

```bash
# Save a baseline from the current codebase (e.g. on `main`):
cargo bench --package whisper-desktop -- --save-baseline main

# After a change, compare:
cargo bench --package whisper-desktop -- --baseline main
```

For CI comparison, archive the `target/criterion` directory as a build
artifact in the CI pipeline. The full raw data can be retrieved from:

```yaml
- name: Upload benchmark artifacts
  uses: actions/upload-artifact@v4
  with:
    name: criterion-reports
    path: desktop/src-tauri/target/criterion/
```

The CI job should **not** run the actual benchmarks (they need a consistent
CPU and a real machine), but saving the baseline from a local or dedicated
runner is fine. CI does not build the benches separately either: that step
cost 16 of the lint job's 23 minutes. Compilation is covered by
`cargo clippy --all-targets`, which includes bench targets.

## Adding new benchmarks

1. Add a new `fn my_bench(c: &mut Criterion)` in
   `desktop/src-tauri/benches/engine_bench.rs`.
2. Register it in the `criterion_group!` macro's target list.
3. Document the benchmark and its budget in this file.
4. Run `cargo clippy --all-targets` to confirm it compiles — that is the
   check CI runs.
