# Privacy and network behavior

Sotto performs local transcription by default. Cloud speech-to-text and LLM formatting are optional and require the user to configure a provider.

## Product telemetry

Telemetry is enabled by default and can be disabled in **Settings → Advanced → Telemetry**.

The Rust process sends a small allow-listed set of de-identified usage events directly to PostHog Cloud EU. Events use a random installation ID; they are not derived from an account, username, hostname, MAC address, path, or hardware fingerprint.

Events include the application version and release channel. Known cloud service names are classified locally from the request endpoint; unknown services are reported as `custom`, without transmitting the endpoint.

Telemetry does not send transcript text, formatted output, prompts, clipboard contents, audio, filenames, filesystem paths, usernames, hostnames, API keys, provider responses, microphone names, focused-window details, or raw errors.

Disabling telemetry stops new capture and delivery; it does not retract events already delivered to the service.

See [telemetry.md](telemetry.md) for the versioned event contract. That file also contains maintainer-only deployment details and is not a substitute for a privacy notice.

## Network requests

The application may contact:

- the configured cloud STT or LLM provider, sending the audio/text required by that provider;
- the model hosting endpoint used by the model downloader;
- GitHub Releases for the startup/manual update check in installed release builds, for an update download after the user requests installation, and for release notes before installation or after an upgrade when they are not cached;
- PostHog Cloud EU for telemetry, when telemetry is enabled in the build.

The application UI uses system fonts and does not load font assets from a third-party CDN.

Review provider settings before enabling a cloud workflow. Do not put secrets, transcripts, recordings, or provider responses into public bug reports.

## Local data

History, settings, telemetry outbox data, and optional diagnostic recordings are stored locally by the application. Diagnostic recording is a separate opt-in setting. To request help, share only the minimum redacted logs needed to reproduce a problem.

Pasting goes through the system clipboard, so the last dictated text stays there until something else is copied, and a clipboard history such as Windows' Win+V may keep it longer.

Model cards take their speed from measurements bundled with the application and record nothing about your own dictations or model loads. Builds up to 0.1.3 kept such timings in the local database; a newer build deletes them on its first launch.

### Where data is stored

| Data | Windows | macOS |
| --- | --- | --- |
| Settings (`config.json`) | `%APPDATA%\com.sotto.app` | `~/Library/Application Support/com.sotto.app` |
| History and statistics database, logs, diagnostic recordings | `%LOCALAPPDATA%\com.sotto.app` | `~/Library/Application Support/com.sotto.app` |
| Downloaded models | `%LOCALAPPDATA%\sotto\models` | `~/Library/Caches/sotto/models` |
| API keys | Credential Manager, entries ending in `.sotto` | Keychain, service `sotto` |

A portable copy keeps everything except API keys in the `data` folder next to `Sotto.exe`; see [Portable version](portable.md).

Builds up to 0.1.3 kept the database, logs and recordings in `~/.speech_to_text` and API keys under the service name `speech-to-text`. A newer build moves the folder on its first launch and moves each key the first time it is used; nothing has to be done by hand. If the database is still open in another copy of the app, the move waits for the next launch.

### Removing all data

On Windows, uninstall Sotto and select the option to delete application data. The uninstaller then removes everything in the table above, including API keys and the folders left by builds up to 0.1.3. Without that option, and during updates, all data is kept.

On macOS, move Sotto to the Trash, then delete the folders in the table above, `~/.speech_to_text` if it still exists, and the `sotto` and `speech-to-text` items in Keychain Access.

For a security vulnerability, use the repository's private vulnerability reporting flow described in [SECURITY.md](../SECURITY.md), not a public issue.

## User-initiated GitHub reports

The Help page can open GitHub issue templates in your browser. A reviewed, optional technical summary is passed in the bug-report URL and can therefore appear in browser history. GitHub receives it when that page opens; publication requires a separate action on GitHub. Suggestions open without diagnostics. This flow works independently of the telemetry switch.

Sanitized logs are prepared locally and saved only to a location you choose. Sotto does not upload them: you attach the reviewed file on GitHub yourself. GitHub uploads selected attachments before issue submission. Public reports must not contain secrets or personal data; see [Troubleshooting](troubleshooting.md#report-a-problem-or-suggest-an-improvement) for the export's contents and limitations.
