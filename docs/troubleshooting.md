# Troubleshooting

## Windows or macOS refuses to open the download

Expected: the builds carry no publisher certificate, so SmartScreen and Gatekeeper cannot say who made them. [Verifying a download](verifying-downloads.md) has the click-through for both systems, plus the checksum and update-signature checks worth doing first.

## The application does not build

Confirm that Rust stable, Node.js LTS, pnpm, CMake, LLVM/libclang, and the platform-specific native toolchain are installed.

Run the frontend and Rust checks separately from their documented working directories in [Development](development.md), which also lists the Windows-specific MSVC, NSIS, WebView2, and `LIBCLANG_PATH` requirements.

## A model is missing or fails to load

Check the model family, platform, cache directory, and available disk space in [Models](models.md). GigaAM v3 is available on Windows and macOS.

A custom Whisper file must be a compatible `.bin` model; do not rename an unrelated model to make it appear in the catalog.

## The first dictation after a pause is slower

The model is unloaded from memory after a period without dictation — five minutes by default — and is loaded again at the start of the next recording. The load runs while you speak, so it is usually invisible; a large model on a slow disk can still delay the transcription that follows.

Change the interval, or switch the behaviour off entirely, under Settings → Advanced → "Unload the model".

While the model is out of memory the sidebar says so and states that it comes back on its own; "No model loaded" means something else — nothing is selected or downloaded.

## Recording seems to start late

There is no intentional countdown before capture. The hotkey's 500 ms auto-repeat guard ignores repeated key-down events while the key is held; it does not postpone the first press. Start dictation with the global hotkey while the target editor has focus. The tray only provides status and settings navigation.

Each recording opens the selected microphone again. Configuration and route checks, the audio worker queue, device setup and the first audio callback all precede captured speech; their duration depends on the device and system. The recording overlay is notified after the stream starts and the start hooks run, so its appearance is not an exact timestamp for the first captured sample. Model restoration is queued after the stream starts and does not wait for inference readiness before capturing audio.

For measurements, use the local `capture timing` log entries described in [Testing](testing.md). `queue_ms` includes start checks before the audio worker runs, and `stream_ready_ms` is measured from the same start request. `first_callback_ms` starts later, just before stream construction: it excludes device selection and configuration, and must not be treated as the full hotkey-to-audio delay or added to `stream_ready_ms`. Compare cold and repeated starts with the same microphone, OS and build; file transcription does not exercise microphone startup.

## The hotkey, microphone, or paste action does not work

Check the operating-system permissions and verify that another application has not claimed the shortcut.

To test delivery without recording or transcription, open **Help → Diagnostics → Test paste**. Switch to an editable field in the target application during the three-second countdown; Sotto copies sample text to the clipboard and attempts to paste it through the dictation delivery path. Check the target field yourself: a successful dispatch does not prove that the application accepted the text.

If macOS dictation reaches the clipboard and manual Cmd+V works, capture and transcription completed; investigate automatic paste separately. Test in a native text editor and a browser field, include the active keyboard layout and the shortcut used to start recording. Sotto sends the physical V key for Cmd+V so Cyrillic layouts do not require switching to English.

Automatic key dispatch cannot confirm that every target application accepted the text. If insertion fails, the clipboard remains available for manual Ctrl+V on Windows or Cmd+V on macOS. On Windows, Sotto only dispatches paste after confirming that the captured target window has focus; a denied focus request leaves the text on the clipboard. Report whether the failure affects all applications or only a particular field; repeated permission changes alone do not diagnose a layout or focus problem.

Sotto normalizes any microphone rate to the 16 kHz the speech models expect, so a 44.1 kHz device no longer produces distorted or wrongly timed audio. A device whose sample format the application cannot read is refused with an error at the start of the recording instead of capturing silence.

Include the OS, architecture, app version, selected model, and whether the failure affects microphone or file transcription when opening an issue. Never include API keys, raw transcripts, or recordings.

## Cloud processing behaves unexpectedly

Cloud STT and LLM formatting are opt-in. Verify the selected provider, endpoint, model, and key in the Integrations settings, then retry with local processing to separate provider failures from the local pipeline.

The LLM timeout in the Integrations settings is the whole budget for one dictation, including retries and the pause between them, not the limit for a single attempt. When it runs out, the transcription is delivered without cloud formatting rather than held back.

## Report a problem or suggest an improvement

Open **Help → Report a problem** to preview a public technical summary and optionally prepare sanitized logs. Continue on GitHub opens a bug-report draft in your browser; a GitHub account is required, and you must submit the issue there. You can exclude the summary or copy it manually. Add your OS version, reproduction steps, expected result and actual result on GitHub.

The sanitized export includes event timestamps, levels, known modules and selected numeric latency measurements from at most the last 256 KiB of the active log. Free-form messages, source locations, paths, recordings and rotated archives are excluded. This conservative export can omit details needed to diagnose an error. Review and save the preview, then attach that file manually on GitHub. GitHub uploads an attachment immediately when selected, before issue submission; reports and attachments are public. Never attach the entire diagnostics folder.

**Suggest an improvement** opens the feature template without diagnostics. **View known issues** opens the tracker so you can check for existing reports. Opening GitHub does not confirm issue publication. No feedback server, Telegram integration or GitHub credentials in Sotto are involved. Report security vulnerabilities through the private flow in [SECURITY.md](../SECURITY.md).
