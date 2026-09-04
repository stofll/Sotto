# Troubleshooting

## The application does not build

Confirm that Rust stable, Node.js LTS, pnpm, CMake, LLVM/libclang, and the
platform-specific native toolchain are installed. Run the frontend and Rust
checks separately from their documented working directories in
[Development](development.md), which also lists the Windows-specific MSVC,
NSIS, WebView2, and `LIBCLANG_PATH` requirements.

## A model is missing or fails to load

Check the model family, platform, cache directory, and available disk space in
[Models](models.md). GigaAM v3 is Windows-only. A custom Whisper file must be a
compatible `.bin` model; do not rename an unrelated model to make it appear in
the catalog.

## The hotkey, microphone, or paste action does not work

Check the operating-system permissions and verify that another application has
not claimed the shortcut. Include the OS, architecture, app version, selected
model, and whether the failure affects microphone or file transcription when
opening an issue. Never include API keys, raw transcripts, or recordings.

## Cloud processing behaves unexpectedly

Cloud STT and LLM formatting are opt-in. Verify the selected provider, endpoint,
model, and key in the Integrations settings, then retry with local processing to
separate provider failures from the local pipeline.
