# Local models

Local recognition runs on two engines, and the difference between them is visible to the user, so the catalog spells it out.

* **Whisper** via `whisper.cpp`: catalog GGML `.bin` files and user-provided `.bin` files placed in the models directory.

  A user-supplied file must be compatible with `whisper.cpp`; the app checks the extension and size but cannot determine the architecture of an arbitrary GGML file before loading it.

  Multilingual builds (`tiny`, `base`, `small`, `medium`, `large-v3`, `turbo`) and English-only ones (`*.en`) sit side by side in the catalog; the language is a property of the model, not part of its name.
* **sherpa-onnx** on Windows and macOS: a closed set of bundles, each with its own family and its own language list. Arbitrary ONNX files are deliberately not picked up: an incompatible graph can throw inside the C++ FFI and crash the process.

## Sherpa families

The engine is selected from the manifest, not from the file name: `ModelEngine` knows which artifact roles each family requires, and the tests use the same knowledge to check manifest completeness.

| Family | Model | Engine | Languages | Size |
|---|---|---|---|---|
| GigaAM | GigaAM v3 | NeMo CTC | ru | 214 MB |
| Omnilingual | Omnilingual 300M | Omnilingual CTC | multilingual, 1600+ | 348 MB |
| Nemotron | Nemotron 3.5 | streaming transducer | multilingual list | 651 MB |
| Canary | Canary 180M Flash | Canary | en | 198 MB |
| Moonshine | Moonshine base | Moonshine | en | 274 MB |
| SenseVoice | SenseVoice small | SenseVoice | zh, en, ja, ko, yue | 229 MB |
| Zipformer | Zipformer | transducer | ru | 70 MB |
| Zipformer | Zipformer small | streaming transducer | ru | 27 MB |
| Parakeet | Parakeet unified | streaming transducer | en | 632 MB |
| Parakeet | Parakeet TDT v2 | transducer | en | 631 MB |
| Parakeet | Parakeet TDT v3 | transducer | multilingual list | 639 MB |

All of them run on the CPU provider of ONNX Runtime. The CPU/GPU switch only applies to Whisper, and the UI does not show it for sherpa models.

Quantization is shown by the catalog next to the size: `int8` for sherpa bundles, `q8_0` for some Whisper builds. It explains why a model weighs less than expected, and it lives next to the size rather than in the name.

### Languages are a closed list

A model asked for a language it does not support does not fail — it silently produces garbage. That is why every bundle records its language list in the manifest, and an incompatible model-plus-language pair is rejected before the FFI call, with a clear message.

`None` in this field means a multilingual model with no restrictions.

### Streaming models

Only `SherpaStreamingTransducer` produces text while you speak; the others stay silent until the recording ends. The live preview in the overlay picks a model by exactly this property. "Streaming" is a property of the model on its catalog card, not part of its name.

Parakeet unified uses the 1120 ms buffered-streaming export. It processes larger chunks than the previous 560 ms export, reducing repeated encoder work at the cost of a later first result and less frequent updates. It still recomputes overlapping audio windows, unlike cache-aware Nemotron. The nominal preset is not a measured first-word latency; see [Benchmarks](benchmarks.md) for throughput measurements and their limits. Existing 560 ms installations must be downloaded again through the model catalog; artifact verification rejects the old encoder, and old speed observations do not carry over to the new revision.

Closing a phrase pads it with 0.6 s of silence before the stream is finished. A cache-aware model decodes a chunk only once the look-ahead behind it has arrived, so a recording that ends on the final syllable loses its last word: «у лукоморье туб» instead of «туб зелёный». The padding stands for the moment of quiet between the last word and the hotkey.

It is fed at the rate the phrase itself came at — handing sherpa a second sample rate aborts the process rather than returning an error.

Nemotron 3.5 is the only streaming model in the catalog that knows both Russian and English, and it detects the language itself: sherpa-onnx does not expose the `prompt_index` input that would pin one.

Its model card says the detected locale is emitted into the transcript as a `<ru-RU>`-shaped tag; through sherpa it is not — checked on Ukrainian, German, Japanese and Spanish samples, none of which produced one.

Its language list holds only what the model card calls transcription-ready and broad-coverage. The eight "adaptation-ready" locales are in its token table but need fine-tuning before they transcribe, so the catalog does not offer them.

### Punctuation

GigaAM v3 comes from the punctuating export upstream publishes alongside the plain one: the same graph, a token table with punctuation marks and capital letters instead of bare lowercase.

Nemotron 3.5 punctuates and capitalises natively as well. For the rest, punctuation is the LLM cleanup step's job.

### Artifact verification

Before calling sherpa, every file of a bundle is checked against a closed SHA-256 registry; URLs are pinned to an upstream commit. An incomplete or modified folder does not count as installed.

The loader assembles a bundle in a staging directory and only then atomically publishes the subfolder — an interrupted download leaves no half-installed model, and the download resumes where it stopped.

A stalled download remains cancellable while waiting for the server or the next data chunk. A connection that does not return response headers within 30 seconds, or sends no new data for 60 seconds, fails with a retryable transport error; the whole download has no fixed duration limit. Network interruptions retain partial progress for retry, while explicit cancellation removes download staging files.

## Speed and download availability

Model cards show one thin speed bar with three fixed fills: one third, two thirds, or full. Higher fill means faster reference throughput; missing measurements use hatching. Hover or keyboard focus explains the relative speed without opening a detail panel. The thresholds are processing taking more than the audio duration, up to the audio duration, and at most a quarter of the audio duration. These categories do not rate accuracy or live-preview latency.

The bundled reference catalog covers all 21 built-in models on a Ryzen 7 5800X CPU with eight threads. Cards use the CPU reference for the exact artifact revision, measured under the language selected in Settings; a language the catalog does not hold falls back to the automatic-detection measurement. The device is always the processor, regardless of the current GPU setting. The reference is a comparative guide, not a predicted duration on the user's hardware. Short prepared-speech samples do not predict all accents, recording lengths or GPU performance. Local observations remain stored separately and never reach the card.

The language is not cosmetic. Whisper runs a detection pass for `auto`, which costs it roughly twice the processing time of an explicit language, and moves `base` and `base.en` a whole level down the bar. Sherpa models do not run that pass and their measurements agree within a few percent. Selecting a language therefore changes what the Whisper cards claim, and the claim follows the setting so the two engine families stay on one honest scale.

The download button remains available when disk space is sufficient or unknown. When known free space is below the full model download size plus the downloader's 1 MiB reserve, the card instead shows “Not enough space”, with required and available disk space on hover or keyboard focus. Free space is checked on the filesystem that will hold the models directory — the nearest existing ancestor answers before the first download creates the directory itself. RAM is not download capacity. Return focus to the app after freeing space to refresh the hint. Already installed models remain selectable. The downloader also performs its own preflight checks because free space may change.

The downloader checks the remaining size of a whole bundle before requesting its first file, and checks again before downloading each file. Verified files and resumable partial downloads reduce the remaining size; zero free bytes means a full disk, while an unavailable measurement permits an attempt. This check does not reserve space against other downloads or applications.

CPU observations survive restarts while the matching profile remains valid. Unverified GPU observations apply only within the current application session because adapter/driver changes cannot currently be identified reliably.

## Models directory

The default location is `%LOCALAPPDATA%/sotto/models` (Linux/macOS use the system cache directory). The path can be overridden with the `SPEECH_TO_TEXT_MODELS_DIR` environment variable.

Before the app was renamed, the directory was called `whisper-desktop`. If it is left over from earlier builds, the app migrates it on the first access to models: nothing downloaded is lost.

If the rename fails (permissions, an open file), work continues with the old directory and the attempt is retried on the next launch.

For Whisper, a compatible `*.bin` file may be placed directly in this directory. For example, a file named `ggml-large-v3-turbo-q5_0.bin` shows up as a user model. Files whose identifiers collide with built-in catalog entries are not published again.

Such a file is removed from the UI just like downloaded ones, but the confirmation warns about the difference: a catalog model can be downloaded again, while a foreign file cannot be recovered from anywhere.

Each sherpa bundle lives in its own subfolder named after the manifest (`gigaam-v3/`, `parakeet-tdt-v3/`, `nemotron-streaming/`, and so on).

When a catalog entry is re-pinned to different artifacts, the installed folder stops matching the manifest, the model shows as not downloaded, and the next download replaces it — nothing has to be deleted by hand.

## Native Sherpa libraries

The `sherpa-onnx` 1.13.7 crate — the Rust API maintained in the Sherpa-ONNX repository itself — pulls in matching binary Sherpa-ONNX/ONNX Runtime libraries during the Cargo build.

Its build script downloads the archive for the target platform unless `SHERPA_ONNX_LIB_DIR` points at one already on disk, and it does not verify what it downloads.

CI therefore never lets it: every build is preceded by `scripts/fetch-sherpa-runtime.ps1` (or the `.sh` twin on macOS), which downloads the archive, checks it against the SHA-256 pinned in `scripts/sherpa-runtime.lock`, and exports `SHERPA_ONNX_LIB_DIR`.

To build locally against verified libraries, set that variable from the same script — it writes the path to standard output, and everything else to standard error:

```powershell
$env:SHERPA_ONNX_LIB_DIR = ./scripts/fetch-sherpa-runtime.ps1
```

```sh
export SHERPA_ONNX_LIB_DIR=$(sh scripts/fetch-sherpa-runtime.sh)
```

Running the script without capturing its output verifies the archive but leaves the variable unset in the calling shell, and the next build downloads its own copy unchecked.

The development build and Windows tests use the libraries from the Cargo target directory.

The Windows Tauri bundle automatically stages the four required DLLs, puts them in the installer resources, and after install/update copies them next to the executable, where the Windows loader finds them.

On macOS, Sherpa and ONNX Runtime are statically linked into the app: no separate `.dylib` files are needed. All families run on CPU; Metal remains a Whisper accelerator.

Linux remains Whisper-only for now: the Sherpa dependency does not link there.

Mac CI verifies the absence of third-party dynamic dependencies with `scripts/check-macos-native-libs.sh` and runs a load, inference, and reopen check for both the regular and the streaming Zipformer.

It can be run from `desktop/src-tauri` with `cargo test --locked --test test_sherpa_runtime -- --ignored`.

The test downloads about 100 MB into a temporary folder, verifies SHA-256, and feeds it silence; recognition quality and the installed DMG are checked separately on a Mac before a release.
