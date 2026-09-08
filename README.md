<div align="center">

<img src="desktop/src-tauri/icons/128x128@2x.png" width="88" alt="Sotto" />

# Sotto

**Write with your voice.**

Sotto turns speech into text and inserts it into your active app. Dictate messages, notes, and prompts with local speech recognition — no account or subscription required.

[![Release](https://img.shields.io/github/v/release/stofll/Sotto?style=flat-square&label=release&color=0b7285)](https://github.com/stofll/Sotto/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/stofll/Sotto/total?style=flat-square&label=downloads&color=0b7285)](https://github.com/stofll/Sotto/releases)
[![Windows](https://img.shields.io/badge/Windows-x64-0b7285?style=flat-square)](docs/platforms.md)
[![macOS](https://img.shields.io/badge/macOS-Apple%20Silicon-0b7285?style=flat-square)](docs/platforms.md)
[![License](https://img.shields.io/github/license/stofll/Sotto?style=flat-square&color=0b7285)](LICENSE)

**[Download Sotto](https://github.com/stofll/Sotto/releases/latest)** · [Features](#features) · [Get started](#get-started) · [Documentation](docs/README.md)

English · [Русский](README.ru.md)

</div>

## Features

### Dictate where you work

Start and stop recording with a hotkey, or hold it for push-to-talk. Send the result straight to the active text field or copy it to the clipboard. Use voice for a quick reply, a longer note, or a detailed prompt.

### Transcribe offline

Download a model once and recognize speech on your computer, even without an internet connection. Choose from Whisper, GigaAM, Parakeet, and other models to suit your language and hardware. Local dictation is free, with no per-minute limits.

See the [model guide](docs/models.md) for languages and download sizes.

### See words as you speak

Streaming models show live transcription in a compact overlay. Follow the text without opening the main window; the overlay also shows the recording and processing state.

### Make the text your own

Add replacements for names, terminology, and recurring recognition mistakes.

For punctuation and formatting, connect an optional LLM and save your own instructions as reusable profiles. Use OpenAI, Anthropic, Gemini, or an OpenAI-compatible service, including local tools such as Ollama and LM Studio.

Cloud speech recognition is also available when configured. External providers may charge for usage.

### Transcribe recordings

Attach an audio file in the **Process the text** panel and get its transcript there. File transcription stays in that panel, separate from dictation history and automatic pasting.

### Revisit what you said

Your dictation history is stored on your computer. Reprocess an entry with a saved LLM profile and review the changes before accepting them. Statistics estimate how much typing time you have saved.

Sound cues, optional audio ducking during recording, and automatic model unloading while idle help dictation fit into your day.

## Get started

### 1. Download and install

Get the latest version from [Releases](https://github.com/stofll/Sotto/releases/latest).

| Platform | Installation |
| --- | --- |
| **Windows x64** | Run the `.exe` installer, or unpack the [portable ZIP](docs/portable.md) and launch `Sotto.exe`. |
| **macOS · Apple Silicon** | Open the `.dmg` and drag Sotto into Applications. Grant microphone and Accessibility permissions when prompted. |

Windows and macOS are supported platforms. Linux is an experimental build target; full functionality is not guaranteed. See [platform support](docs/platforms.md) for architecture details and limitations.

Builds currently lack a publisher certificate, so Windows or macOS may warn on first launch.

See [verifying a download](docs/verifying-downloads.md) for file checks and first-launch instructions, or [troubleshooting](docs/troubleshooting.md) if installation fails.

Installed builds support automatic updates with signature verification; portable copies are updated manually.

### 2. Choose a model

Open **Models**, download a model for your language, and select it for dictation. The catalog shows each model's languages and size. For example, GigaAM v3 recognizes Russian, while Whisper and Parakeet TDT v3 offer multilingual options.

### 3. Dictate your first phrase

Place the cursor in a text field and use your configured hotkey:

- **Toggle mode:** press to start, speak, then press again to finish.
- **Push-to-talk:** hold while speaking and release to finish.

## Privacy

Local recognition is the default. Audio is sent to a cloud speech provider only when you configure and use one; cloud LLM processing sends text to your chosen provider.

Product telemetry is enabled by default and can be disabled in **Settings → Advanced**. It contains no audio, transcripts, or dictated text. See [Privacy](docs/privacy.md) and [Telemetry](docs/telemetry.md) for details.

## Documentation and contributing

- [Documentation](docs/README.md) — models, platforms, privacy, and troubleshooting.
- [Development](docs/development.md) — prerequisites and building from source.
- [Contributing](CONTRIBUTING.md) and [Testing](docs/testing.md) — contributing changes and required checks.
- [Support](SUPPORT.md) — questions and bug reports; [Security](SECURITY.md) — private vulnerability reporting.

## License

Sotto is free and open source under the [MIT License](LICENSE).

Third-party components and speech models have their own license terms; dependency and license inventories are attached to releases. Provider names and logos belong to their respective owners and do not imply affiliation or endorsement.
