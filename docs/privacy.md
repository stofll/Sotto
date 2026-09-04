# Privacy and network behavior

Sotto performs local transcription by default. Cloud speech-to-text and LLM
formatting are optional and require the user to configure a provider.

## Product telemetry

Telemetry is enabled by default and can be disabled in **Settings → Advanced →
Telemetry**. The Rust process sends a small allow-listed set of de-identified
usage events directly to PostHog Cloud EU. Events use a random installation ID;
they are not derived from an account, username, hostname, MAC address, path, or
hardware fingerprint.

Telemetry does not send transcript text, formatted output, prompts, clipboard
contents, audio, filenames, filesystem paths, usernames, hostnames, API keys,
provider responses, microphone names, focused-window details, or raw errors.
Disabling telemetry stops new capture and delivery; it does not retract events
already delivered to the service.

See [telemetry.md](telemetry.md) for the versioned event contract. That file
also contains maintainer-only deployment details and is not a substitute for a
privacy notice.

## Optional network requests

Depending on the features a user enables, the application may contact:

- the configured cloud STT or LLM provider, sending the audio/text required by
  that provider;
- the model hosting endpoint used by the model downloader;
- PostHog Cloud EU for telemetry, when telemetry is enabled in the build.

The application UI uses system fonts and does not load font assets from a
third-party CDN.

Review provider settings before enabling a cloud workflow. Do not put secrets,
transcripts, recordings, or provider responses into public bug reports.

## Local data

History, settings, telemetry outbox data, and optional diagnostic recordings are
stored locally by the application. Diagnostic recording is a separate opt-in
setting. To request help, share only the minimum redacted logs needed to
reproduce a problem.

For a security vulnerability, use the repository's private vulnerability
reporting flow described in [SECURITY.md](../SECURITY.md), not a public issue.
