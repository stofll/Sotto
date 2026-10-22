# Sotto's Whisper model loader backport

This is the published `whisper-rs-sys 0.13.1` crate, containing Whisper.cpp 1.7.4, with a local security patch. The original crate archive has SHA-256 `1c86f1b993f216594b1ad9a9bb00a26014fb7c512e12664a2d401c7897d2ef7d`; its upstream source revision is `b202069aa891d8243206f89599c04f0e8e6a3d27`, subdirectory `sys`, as recorded in `.cargo_vcs_info.json`. Source: [crates.io](https://crates.io/crates/whisper-rs-sys/0.13.1), [upstream Rust wrapper](https://codeberg.org/tazz4843/whisper-rs/src/commit/b202069aa891d8243206f89599c04f0e8e6a3d27/sys).

The wrapper's `Unlicense` declaration and the native MIT license in `whisper.cpp/LICENSE` are preserved. Included upstream files are unchanged except `build.rs` and `whisper.cpp/src/whisper.cpp`; this provenance document is additional. Generated files, Cargo registry checksum metadata and the unused crate-local `Cargo.lock` are omitted. The application's checked-in `Cargo.lock` controls the actual dependency graph.

## Scope and upstream basis

The patch keeps the existing Rust API, native API, inference algorithms, GGML sources, CPU baseline and GPU backends. It hardens the GGML model loader used by file and buffer entry points, bounds reflective padding for audio shorter than 201 samples, and fixes native source invalidation in the build script. Normal-length audio uses the original padding operation. It does not import newer inference kernels or claim the performance improvements of a newer Whisper.cpp release.

The following upstream changes establish the underlying fixes; their guards are adapted to this older loader rather than applying unrelated native updates:

- [Reject out-of-bounds tensor dimensions](https://github.com/ggml-org/whisper.cpp/commit/df1547b6fe362556e48fd21f386c6eee85b971fe).
- [Reject invalid tensor types](https://github.com/ggml-org/whisper.cpp/commit/60c0be6ac8fa71b1a2ae2dd938a31a34a508e774).
- [Keep model-loading exceptions inside the C ABI](https://github.com/ggml-org/whisper.cpp/commit/610e664ba7cfe3af46125ed1b5a1184fccb51bcd).
- [Reject vocabularies that omit required special tokens](https://github.com/ggml-org/whisper.cpp/commit/b29763ed3f366fefe63dba8cc000aa538a4e9c37).
- [Avoid null buffer pointer arithmetic/copies](https://github.com/ggml-org/whisper.cpp/commit/a722846cb60859d0ed140559df3b45a2c2212a88).
- [Bound reflective padding for short audio](https://github.com/ggml-org/whisper.cpp/commit/8631825d41a2712268813981a9550b04a3f225e5), with an additional zero-count guard before constructing sample pointers.

[Upstream PR 3780](https://github.com/ggml-org/whisper.cpp/pull/3780) describes the separate mel-filter multiplication overflow and remains open. This patch checks the exact FFT width required by the existing spectrogram code, 201, and requires the filter count to match the model before performing an unsigned size calculation.

## Additional validation

Every scalar and payload read must return its complete length. EOF is accepted only before a new tensor record; partial headers and truncated filters, vocabulary, names or weights fail. The file callback reports `gcount()`, and the buffer callback computes remaining bytes without addition overflow. Exceptions, including allocation failures inside model loading, return a null context and release partially allocated model resources.

The architecture is bounded before allocation or head-count division: audio context 1–1500, text context 10–448, equal positive encoder/decoder width at most 1280, positive head counts that divide their widths, 1–32 encoder/decoder layers, and 80 or 128 mel bands. These cover the original Whisper architectures and smaller/distilled exports, including large-v3 and turbo; they intentionally reject architectures outside these bounds. The text-context minimum follows the existing decoder prompt's requirement for up to half a context of previous tokens plus five control tokens. No exact model-name or architecture tuple is required. Quantized widths must fit their declared block layout.

Vocabulary size is restricted to the runtime's supported special-token/language tables, 51864–51866; serialized vocabulary entries cannot exceed that count. A token may use at most 1 MiB and the complete serialized vocabulary at most 16 MiB, substantially above the original byte-level BPE vocabulary. These bounds limit metadata allocation; they do not promise that a valid multi-gigabyte model can load on a machine without enough memory.

Tensor dimensions must be 1–4, and all four extents must equal the allocated model tensor's shape. The file's type must equal that tensor's expected type before the loader accesses type traits; removed enum values and equal-sized incompatible layouts are rejected. Names are nonempty, contain no NUL, and cannot exceed the longest actual tensor name. Every expected tensor must appear exactly once. Empty test-only models, missing weights, duplicate records and trailing partial records are rejected.

`build.rs` tracks the native source tree, verifies that its previous copied source directory stays within Cargo's `OUT_DIR`, and replaces that copy whenever the script reruns. This prevents an incremental build from silently retaining an older security patch.

## Regression checks

`tests/test_whisper_model_parser.rs` in the application invokes file and buffer loading on synthetic malformed models in bounded child processes. It covers incomplete reads, architecture bounds, quantization alignment, filter dimensions, vocabulary lengths and aggregate budget, tensor dimensions/types/names/shapes, duplicate/missing weights and trailing records. Native crashes and hangs fail the parent test instead of ending the test runner.

The pre-patch failures were reproduced by linking isolated probes to the original native libraries: a complete header/filter section with no tensors returned a non-null context, as did a file containing the expected record count entirely replaced by duplicate bias records. Both inputs must now return an error. A one-sample PCM buffer immediately before a Windows guard page caused an access violation in `whisper_pcm_to_mel`; bounded reflective padding must complete successfully. The asset-backed `test_whisper_engine` test also calculates spectrograms for 0, 1, 2, 199, 200, 201 and 202 samples before checking repeated real speech inference. Isolated prepared-speech benchmarks verify compatibility with real models; runtime measurements and platform limitations are reported separately. This patch is not a general sandbox for running untrusted inference code or arbitrary native backends.
