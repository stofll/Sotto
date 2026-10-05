# Privacy and network behavior

Sotto performs local transcription by default. Cloud speech-to-text and LLM formatting are optional and require the user to configure a provider.

## Product telemetry

Telemetry is off until you allow it. Turning it on in **Settings → Advanced → Telemetry** or in the [first-launch introduction](onboarding.md) first shows an explanation of the events; only **Turn on** in that explanation enables it. Turning it off takes effect at once.

Leaving the introduction's last step records your answer, even if the box stays unticked. An installation without an answer, such as one that skipped the introduction or was installed before telemetry became opt-in, sends nothing and is asked once in the main window after a successful dictation. **Allow** and **Don't send** store the answer; closing the card hides it until the next launch.

The Rust process sends a small allow-listed set of de-identified usage events directly to PostHog Cloud EU. Events use a random installation ID; they are not derived from an account, username, hostname, MAC address, path, or hardware fingerprint.

Events include the application version and release channel. Known cloud service names are classified locally from the request endpoint; unknown services are reported as `custom`, without transmitting the endpoint.

Telemetry does not send transcript text, formatted output, prompts, clipboard contents, audio, filenames, filesystem paths, usernames, hostnames, API keys, provider responses, microphone names, focused-window details, or raw errors.

Disabling telemetry stops new capture and delivery; it does not retract events already delivered to the service.

See [telemetry.md](telemetry.md) for the versioned event contract. That file also contains maintainer-only deployment details and is not a substitute for a privacy notice.

## Network requests

The application may contact:

- the configured cloud STT or LLM provider, sending the audio/text required by that provider;
- the model hosting endpoint used by the model downloader;
- GitHub Releases for automatic and manual update checks in installed release builds, for an update download after the user requests installation, and for release notes before installation or after an upgrade when they are not cached;
- PostHog Cloud EU for telemetry, when the build includes an ingest token and you have allowed telemetry.

The application UI uses system fonts and does not load font assets from a third-party CDN.

Update checks use stable releases by default. Enabling **Help → Updates → Receive beta builds** also queries the public GitHub Releases list to find newer published betas; the choice is saved locally. Update checks send no audio, transcripts, provider credentials, or application history, and installation still requires an explicit click.

While the settings window is visible and active, Sotto checks for updates on opening and every six hours. A quiet notice at the top of the window appears at most once every 24 hours, waits until dictation and open dialogs finish, and disappears after 12 seconds; hovering or focusing its controls pauses dismissal. **Details** opens the existing update controls in Help.

The reminder timestamp is saved locally in settings so restarting the app does not repeat it immediately. Debug and portable builds do not check for installable updates.

Review provider settings before enabling a cloud workflow. Do not put secrets, transcripts, recordings, or provider responses into public bug reports.

## LLM prompts

The System prompt field on the AI page contains all instructions Sotto sends to the selected LLM profile. You can edit every rule; Sotto does not append mandatory instructions. The source text is sent separately inside a `<dictation>` block. The built-in presets use English instructions and preserve the language of each source passage, including mixed-language text.

Save your prompt before testing it. Saved custom prompts survive updates unchanged; **Restore the built-in** replaces a custom prompt with the current preset and resumes receiving preset updates. The paragraph and list presets are starting points, and all their rules remain editable.

Versions up to 0.3.2-beta.1 appended fixed response and data-boundary rules to every prompt. A custom prompt saved in those versions no longer receives them, so copy the relevant rules from a preset if you need them.

Custom templates can still use `{{language}}`, `{{app}}` and `{{datetime}}`. Legacy `{{text}}` and `{{transcript}}` markers are removed from the system prompt; the source text is always sent in the separate message. An empty prompt skips LLM processing and inserts the local transcript. Response handling still removes reasoning blocks and falls back to the source text for empty responses, comments about the text or excessive word loss; changing the prompt does not disable these checks.

The response is also rejected when it changes the meaning: it has fewer negations than the source, drops a number from it (amounts and list numbers included, while thousands separators and decimal commas may change), or alters a Latin-script name inside mostly Russian text. History names the reason. These checks compare words, not meaning, so they cannot catch every distortion, and a legitimate correction of a recognised English term in Russian text also falls back to the local transcript.

## Local data

History, settings, telemetry outbox data, and optional audio recordings are stored locally by the application. To request help, share only the minimum redacted logs needed to reproduce a problem.

Audio recordings are off by default. When **Settings → Advanced → Save audio recordings** is on, every dictation is kept as a WAV file in the `recordings` folder and can be played from History. The newest 50 are kept unless you choose another limit.

A recording is deleted with its History entry: when you delete the entry, clear History, or the History retention limits remove it. A cancelled dictation's recording is deleted as well. A dictation that produced no text or failed to transcribe has no History entry, so its recording stays in the folder until the recordings limit removes it or you delete it there.

The folder is separate from the logs, so sharing the logs folder does not share your voice. Builds before this change kept recordings inside `logs`, and the first launch of a newer build moves them out.

Pasting goes through the system clipboard, so the last dictated text stays there until something else is copied, and a clipboard history such as Windows' Win+V may keep it longer.

Model cards take their speed from measurements bundled with the application and record nothing about your own dictations or model loads. Builds up to 0.1.3 kept such timings in the local database; a newer build deletes them on its first launch.

### Where data is stored

| Data | Windows | macOS |
| --- | --- | --- |
| Settings (`config.json`) | `%APPDATA%\com.sotto.app` | `~/Library/Application Support/com.sotto.app` |
| History and statistics database, logs, audio recordings | `%LOCALAPPDATA%\com.sotto.app` | `~/Library/Application Support/com.sotto.app` |
| Downloaded models | `%LOCALAPPDATA%\sotto\models` | `~/Library/Caches/sotto/models` |
| API keys | Credential Manager, entries ending in `.sotto` | Keychain, service `sotto` |

A portable copy keeps everything except API keys in the `data` folder next to `Sotto.exe`; see [Portable version](portable.md).

Automatic migration from builds up to 0.1.3 has been retired. The app no longer discovers or moves `~/.speech_to_text` automatically, imports Python-era `stats.json` / `history.json`, or retrieves keys from the `speech-to-text` service. Those sources remain untouched during normal startup; existing data in the current locations continues to work.

If you still use one of those old builds, first launch [v0.3.1](https://github.com/stofll/Sotto/releases/tag/v0.3.1), which includes the migration. Check that your history and downloaded models are available, and open provider settings to verify each stored key: keys migrate when read. Then upgrade to this build. Otherwise, transfer data manually and enter your API keys again; see [Models](models.md#models-directory) for reusing old downloads.

### Removing all data

On Windows, uninstall Sotto and select the option to delete application data. The uninstaller then removes everything in the table above, including API keys and the folders left by builds up to 0.1.3. Without that option, and during updates, all data is kept.

On macOS, move Sotto to the Trash, then delete the folders in the table above, `~/.speech_to_text` if it still exists, and the `sotto` and `speech-to-text` items in Keychain Access.

For a security vulnerability, use the repository's private vulnerability reporting flow described in [SECURITY.md](../SECURITY.md), not a public issue.

## User-initiated GitHub reports

The Help page can open GitHub issue templates in your browser. A reviewed, optional technical summary is passed in the bug-report URL and can therefore appear in browser history. GitHub receives it when that page opens; publication requires a separate action on GitHub. Suggestions open without diagnostics. This flow works independently of the telemetry switch.

Sanitized logs are prepared locally and saved only to a location you choose. Sotto does not upload them: you attach the reviewed file on GitHub yourself. GitHub uploads selected attachments before issue submission. Public reports must not contain secrets or personal data; see [Troubleshooting](troubleshooting.md#report-a-problem-or-suggest-an-improvement) for the export's contents and limitations.
