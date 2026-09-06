# Local models

Local recognition runs on two engines, and the difference between them is
visible to the user, so the catalog spells it out.

* **Whisper** via `whisper.cpp`: catalog GGML `.bin` files and user-provided
  `.bin` files placed in the models directory. A user-supplied file must be
  compatible with `whisper.cpp`; the app checks the extension and size but
  cannot determine the architecture of an arbitrary GGML file before loading
  it. Multilingual builds (`tiny`, `base`, `small`, `medium`, `large-v3`,
  `turbo`) and English-only ones (`*.en`) sit side by side in the catalog;
  the language is a property of the model, not part of its name.
* **sherpa-onnx** on Windows and macOS: a closed set of bundles, each with its
  own family and its own language list. Arbitrary ONNX files are deliberately
  not picked up: an incompatible graph can throw inside the C++ FFI and crash
  the process.

## Sherpa families

The engine is selected from the manifest, not from the file name: `ModelEngine`
knows which artifact roles each family requires, and the tests use the same
knowledge to check manifest completeness.

| Family | Model | Engine | Languages | Size |
|---|---|---|---|---|
| GigaAM | GigaAM v3 | NeMo CTC | ru | 214 MB |
| Canary | Canary 180M Flash | Canary | en | 198 MB |
| Moonshine | Moonshine base | Moonshine | en | 274 MB |
| SenseVoice | SenseVoice small | SenseVoice | zh, en, ja, ko, yue | 229 MB |
| Zipformer | Zipformer | transducer | ru | 70 MB |
| Zipformer | Zipformer small | streaming transducer | ru | 27 MB |
| Parakeet | Parakeet unified | streaming transducer | en | 632 MB |
| Parakeet | Parakeet TDT v2 | transducer | en | 631 MB |
| Parakeet | Parakeet TDT v3 | transducer | multilingual list | 639 MB |

All of them run on the CPU provider of ONNX Runtime. The CPU/GPU switch only
applies to Whisper, and the UI does not show it for sherpa models.

Quantization is shown by the catalog next to the size: `int8` for sherpa
bundles, `q8_0` for some Whisper builds. It explains why a model weighs less
than expected, and it lives next to the size rather than in the name.

### Languages are a closed list

A model asked for a language it does not support does not fail — it silently
produces garbage. That is why every bundle records its language list in the
manifest, and an incompatible model-plus-language pair is rejected before the
FFI call, with a clear message. `None` in this field means a multilingual
model with no restrictions.

### Streaming models

Only `SherpaStreamingTransducer` produces text while you speak; the others
stay silent until the recording ends. The live preview in the overlay picks
a model by exactly this property. "Streaming" is a property of the model on
its catalog card, not part of its name.

### Artifact verification

Before calling sherpa, every file of a bundle is checked against a closed
SHA-256 registry; URLs are pinned to an upstream commit. An incomplete or
modified folder does not count as installed. The loader assembles a bundle in
a staging directory and only then atomically publishes the subfolder — an
interrupted download leaves no half-installed model, and the download resumes
where it stopped.

## Models directory

The default location is `%LOCALAPPDATA%/sotto/models` (Linux/macOS use the
system cache directory). The path can be overridden with the
`SPEECH_TO_TEXT_MODELS_DIR` environment variable.

Before the app was renamed, the directory was called `whisper-desktop`. If it
is left over from earlier builds, the app migrates it on the first access to
models: nothing downloaded is lost. If the rename fails (permissions, an open
file), work continues with the old directory and the attempt is retried on
the next launch.

For Whisper, a compatible `*.bin` file may be placed directly in this
directory. For example, a file named `ggml-large-v3-turbo-q5_0.bin` shows up
as a user model. Files whose identifiers collide with built-in catalog entries
are not published again.

Such a file is removed from the UI just like downloaded ones, but the
confirmation warns about the difference: a catalog model can be downloaded
again, while a foreign file cannot be recovered from anywhere.

Each sherpa bundle lives in its own subfolder named after the manifest
(`gigaam-v3/`, `parakeet-tdt-v3/`, `zipformer-ru-streaming/`, and so on).

## Native Sherpa libraries

The `sherpa-rs` 0.6.8 crate pulls in pinned binary Sherpa-ONNX/ONNX Runtime
libraries during the Cargo build. The development build and Windows tests use
them from the Cargo target directory. The Windows Tauri bundle automatically
stages the five required DLLs, puts them in the installer resources, and after
install/update copies them next to the executable, where the Windows loader
finds them. On macOS, Sherpa and ONNX Runtime are statically linked into the
app from a pinned universal2 archive: no separate `.dylib` files are needed.
All families run on CPU; Metal remains a Whisper accelerator.
Linux remains Whisper-only for now: the Sherpa dependency does not link there.

Mac CI verifies the absence of third-party dynamic dependencies with
`scripts/check-macos-native-libs.sh` and runs a load, inference, and reopen
check for both the regular and the streaming Zipformer. It can be run from
`desktop/src-tauri` with
`cargo test --locked --test test_sherpa_runtime -- --ignored`.
The test downloads about 100 MB into a temporary folder, verifies SHA-256,
and feeds it silence; recognition quality and the installed DMG are checked
separately on a Mac before a release.
