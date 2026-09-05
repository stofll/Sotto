# Sotto

Local voice dictation for Windows, with a macOS port. Press a hotkey, speak, and
the text appears in whatever window you were typing in.

```text
hotkey -> record voice -> local transcription -> optional LLM formatting -> paste/copy result
```

Transcription runs fully on-device by default. Cloud speech-to-text and LLM text
formatting exist, but they are opt-in: nothing leaves the machine until you
configure a provider yourself. See [Privacy](docs/privacy.md).

## Install

Download the latest build from [Releases](../../releases/latest).

**Windows** — run the `Sotto_<version>_x64-setup.exe` installer. Windows 11 ships
the WebView2 runtime the app needs; on Windows 10 the installer will prompt for
it. SmartScreen warns about an unknown publisher: **More info** → **Run anyway**.

**macOS** — open the DMG and drag Sotto to Applications. Gatekeeper blocks the
first launch; allow it in **System Settings** → **Privacy & Security** →
**Open Anyway**. The port is functional but less exercised than Windows — see
[Platform support](docs/platforms.md) for what is verified on each target.

Both warnings are honest: the builds are not signed with a publisher
certificate (no Authenticode, no Apple Developer ID), so neither system can
tell who produced them. What you can check yourself is that the file is the one
the release workflow produced — every release ships `SHA256SUMS.txt`, and
update artifacts are signed with a minisign key. See
[Verifying a download](docs/verifying-downloads.md).

The app updates itself: releases are signed with that minisign key and verified
before installation.

## What it does

- **Dictation by hotkey** into the focused window, with an overlay showing state.
- **Local speech models** — `whisper.cpp` everywhere, plus `sherpa-onnx` bundles
  on Windows and macOS (GigaAM, Parakeet, Canary, Moonshine, SenseVoice, Zipformer). Two of
  them stream text while you are still speaking. See [Models](docs/models.md).
- **File transcription** — attach a recording in the "Обработать текст" panel and
  the text comes back there, without touching the focused window or the history.
- **Optional LLM cleanup** — punctuation, formatting, and custom prompts through
  OpenAI-compatible providers.
- **Text rules** — replacements and dictionaries applied before the result lands.
- **History and statistics**, stored locally in SQLite.

## Privacy

Speech is transcribed on your machine. Recordings are not uploaded anywhere
unless you explicitly enable a cloud provider.

De-identified product telemetry is on by default and can be turned off in
Settings → Advanced. It carries no audio, no transcripts, and no text you
dictate. [Telemetry](docs/telemetry.md) documents every event and property that
is sent; [Privacy](docs/privacy.md) covers the full network picture.

## Current Status

- Version: `0.0.3`
- Desktop shell: **Tauri 2 + Rust** backend, **React 19 / TypeScript / Vite** frontend
- Speech backend: **`whisper-rs`** (whisper.cpp, GGML models) — fully native, no Python
- Second speech backend (Windows/macOS): **`sherpa-onnx`**, CPU-only, each bundle pinned
  to an exact artifact manifest and to its own list of languages
- Data: SQLite (`rusqlite`, bundled) for history and statistics
- Sections: Settings, Models, Text, LLM processing, Integrations, History, Statistics

## Documentation

Start at the [documentation index](docs/README.md).

- [Platform support](docs/platforms.md) — what is verified where
- [Models](docs/models.md) — model families, storage, platform limits
- [Troubleshooting](docs/troubleshooting.md) — installation and runtime issues
- [Privacy](docs/privacy.md) and [Telemetry](docs/telemetry.md)

## Building from source

Prerequisites, the development loop, and the repository layout are in
[Development](docs/development.md). The checks expected before a pull request are
in [Testing](docs/testing.md).

```bash
cd desktop
pnpm install --frozen-lockfile
pnpm tauri dev
```

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first. Security reports go through
[SECURITY.md](SECURITY.md) rather than a public issue. For questions, see
[SUPPORT.md](SUPPORT.md).

## License

[MIT](LICENSE).

The license covers this repository's own source. Bundled and downloaded
components — speech models, native runtimes, provider logos — carry their own
terms. A per-release inventory of dependencies and their licenses is generated
by the SBOM workflow and attached to each release as `sbom-rust.cdx.json`,
`sbom-npm.cdx.json` and `licenses-npm-*.json`.

Provider names and logos shown in the interface are trademarks of their
respective owners and identify optional integrations only; no affiliation,
sponsorship or endorsement is implied.
