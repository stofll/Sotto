# Product telemetry

> **Maintainer reference.** This document records the implementation contract and deployment controls. For the user-facing privacy summary, use [privacy.md](privacy.md). Keep ingest tokens and other secrets out of the repository, issues, and pull requests.

Sotto sends privacy-minimized product events from the Rust process directly to PostHog Cloud EU. There is no browser analytics SDK, UI autocapture, session replay, exception capture, or click tracking.

Telemetry is enabled by default and can be disabled in **Settings → Advanced → Telemetry**. Disabling takes effect immediately for new capture and delivery.

It does not remove already delivered events or delete the durable local outbox. Re-enabling resumes delivery of pending rows.

Both settings live in `config.json` (`telemetry_enabled`, `telemetry_session_timeout_minutes`) and are written through the ordinary `save_config` merge patch, which re-syncs the live capture gate before it returns — there is no separate telemetry command and no restart is needed.

Only the consent switch has a control in the UI: the session timeout is an aggregation parameter with nothing for a user to decide, so it is set by editing `config.json` and otherwise keeps its default.

A timeout outside the supported range is clamped, never rejected: a hand-edited config must not make unrelated settings unsavable.

## Release configuration

The public PostHog project ingest token is compiled into a release build:

```powershell
$env:SOTTO_POSTHOG_API_KEY = "phc_..."
pnpm tauri build
```

`POSTHOG_API_KEY` is accepted as a fallback build variable for release builds. Debug builds accept only `SOTTO_POSTHOG_DEV_API_KEY`, which must point to a separate development project; they never fall back to the production token. This must be a public project ingest token, never a PostHog personal or administrative API key.

Development telemetry also requires an isolated `SOTTO_DATA_DIR` for the manual run. A worktree or debug executable does not isolate SQLite: sharing the production directory would share both the installation ID and pending outbox rows across projects. The variable moves the database and logs only; settings are still read from the configuration directory listed in [Privacy](privacy.md#where-data-is-stored). Never point a development run at the user's live application data.

Because the token is a compile-time input, a build that misses it cannot be repaired at runtime, and nothing about the running app reveals the difference.

`scripts/build-installer.sh`, for builds made outside CI, therefore takes the token from `~/.tauri/sotto-posthog.key`, refuses to build without one, and verifies afterwards that the ingest host actually survived into the artifact.

`SOTTO_ALLOW_NO_TELEMETRY=1` waives both checks, for a build that is meant to report nothing.

The release workflow reads the same variable from a repository secret. It has no equivalent guard and can ship a build without telemetry if the secret is absent. Check the repository's release configuration rather than treating this document as evidence that a secret is present. See [RELEASE.md](RELEASE.md).

Without a token no events are queued and neither the delivery worker nor the session watcher starts. Initialization still reads or creates the random installation ID in SQLite; it does not eliminate all database access.

The v1 endpoint is fixed to `https://eu.i.posthog.com/capture/`. Every event sets `$process_person_profile: false` and `$geoip_disable: true`.

In the PostHog project, also disable IP capture and keep autocapture, session replay, exception capture, and person profiles off.

## Identity and storage

The application generates a random UUIDv4 installation ID and stores it in SQLite table `telemetry_meta`. It is not derived from an account, username, hostname, MAC address, path, or hardware fingerprint. The ID is used as PostHog `distinct_id` so unique installations and retention can be measured.

Events first enter `telemetry_outbox`. Delivery preserves the original event time as an ISO 8601 UTC `timestamp`, including after offline delivery, retries and process restarts. Delivery uses a stable `$insert_id`, a 15-second HTTP timeout, bounded exponential retry for network errors, HTTP 429 and 5xx responses, and drops permanent 4xx payload errors.

The outbox retains at most 1,000 rows and seven days, deleting the oldest rows first; that housekeeping runs on the delivery tick rather than on every insert, so recording an event costs one `INSERT`.

Telemetry failure never blocks or fails recording, transcription, paste, or startup.

Every event also carries `usage_session_id`, the random id of the usage session it belongs to, so per-session insights can be built without correlating on timestamps.

## Event contract (schema version 1)

All events contain `schema_version`, `app_version`, `release_channel`, `os`, `os_major`, and `arch`. `release_channel` is `dev` for debug builds, `beta` for release builds with a SemVer prerelease suffix, and `stable` otherwise; build metadata alone does not make a prerelease. Older events omit it, so do not classify missing values as stable. `os_major` is currently `unknown`; it is reserved so OS-version collection can be added without changing the event schema.

### `app.started`

Measures launches and version adoption.

- `start_mode`: `interactive` or `autostart`
- `ui_language`: `ru`, `en`, or `other`

An app launch does not start a usage session because a tray process may stay open without being used.

### `transcription.completed`

Emitted once for a successful microphone or file transcription. A paste failure is still a completed transcription and is represented by `paste_result: failed`.

- `source`: `microphone` or `file`
- `pipeline_mode`: `local`, `hybrid`, `cloud`, or `other`
- `recording_mode`: `push_to_talk`, `toggle`, or `not_applicable`
- `stt_engine`: `whisper`, `sherpa`, `cloud_stt`, or `other`
- `stt_provider`: `local` or the fixed `compatible` adapter for completed cloud operations
- `stt_model`: canonical local ID from the application's model catalogue (including all Whisper and Sherpa models available on the platform), `custom_local` for models outside the catalogue, or a sanitized/capped cloud model label. Known aliases are normalized to the catalogue ID.
- `stt_model_name`: display name from the same catalogue, such as `SenseVoice small` or `Parakeet unified`. Omitted for unknown local models and cloud STT; never derived from a user filename or path.
- `compute`: `cpu`, `gpu`, `cloud`, or `other` — `other` means the route never reached the engine, which is what every failure reports
- numeric `audio_seconds`, `processing_seconds`, and `time_saved_seconds`
- bucketed `output_length_bucket`
- `llm_attempted`, `llm_used`, `llm_fallback`, allow-listed provider, and a sanitized/capped model label only after the provider returned a response (plus the allow-listed fallback reason)
- `paste_result`, `formatting_enabled`, and a replacement-count bucket

Audio is rounded to 10 seconds; processing time, estimated time saved, and active session duration are rounded to one second. The estimate currently uses the same 240 characters per minute fallback as the local statistics writer.

### `transcription.failed`

Emitted once for a failed microphone or file operation. `stage` and `reason` are fixed low-cardinality values. Raw errors, HTTP bodies, URLs, or exception messages never enter the event.

Failures use the same typed outcome envelope, with unavailable model fields omitted and duration counters left at zero. `reason_detail` preserves the specific allow-listed cause (for example, `engine_busy`, `decode` or `empty_transcript`) while the existing coarse `reason` remains compatible with older dashboards. These defaults are not measurements: use completed events for latency and volume, and do not attribute a failed request to the currently selected model.

### `transcription.cancelled`

Emitted separately so deliberate cancellation is not counted as a reliability failure. Covers both microphone and file transcription.

### `usage_session.finished`

A usage session begins with a real microphone/file attempt or an explicit LLM utility action. It ends after the configured inactivity timeout (30 minutes by default, configurable from 5 to 120 minutes) or on orderly application exit.

A watcher checks inactivity every 15 seconds.

The event contains active duration through the last action—not the idle timeout—plus transcription, success, failure and cancellation counts, rounded audio/time-saved totals, dominant pipeline mode, and the effective timeout. `dominant_pipeline_mode` is `other` when the session has no observed local, hybrid or cloud transcription, including sessions containing only LLM utility actions.

An in-flight long transcription prevents the idle watcher from splitting that single operation into two sessions.

## Provider services

`stt_provider` and `llm_provider` retain their existing adapter meaning. New `stt_service` and `llm_service` fields distinguish a fixed list of known services by parsing the actual request endpoint locally; unmatched endpoints become `custom`. Host matching is exact, not substring-based. Endpoints, credentials and user profile names never enter an event.

The list covers every provider preset the app ships, so `custom` means a self-hosted or hand-entered endpoint rather than a service the allowlist forgot. Adding a preset without adding its host silently merges that traffic into `custom`.

Local servers (LM Studio, Ollama, vLLM) stay `custom` by design: their host is `localhost`, and nothing about it is worth transmitting.

Cloud STT carries the service from the successful request through inference into both microphone and file outcomes. LLM service is reported only for an attempted/used/fallback operation and follows the adapter's effective endpoint, including Gemini's fixed endpoint. These internal fields are skipped in ordinary IPC/history serialization. Earlier events cannot recover service labels, and STT failures that have no request context still omit the service.

## Data that must never be sent

The telemetry API is typed and allow-listed. Do not add a generic event name or arbitrary JSON capture method. Payloads must never include:

- transcript, formatted output, prompt, or clipboard contents;
- audio samples or recordings;
- filenames, filesystem paths, usernames, or hostnames;
- API keys, custom endpoints, provider responses, or raw errors;
- microphone names, focused-window details, or application titles.

The application model catalogue is the allowlist for local STT IDs, display names, and engines; discovered local model names become `custom_local` with no display name.

A cloud STT model is captured only after a completed operation. An LLM model is captured only after the provider returned a response; a rejected request retains its allow-listed provider but not the unvalidated model string.

Values are normalized to stable allowlists or bounded ASCII labels. Invalid, path-like, URL-like, key-like, or otherwise suspicious values become `custom_cloud`/`custom_llm` and are never forwarded verbatim. No base URL or arbitrary local filename is sent.

## PostHog dashboard

Use three focused views: a product overview (active installations, last observed versions, first successful use and retention), usage (models/adapters, unique installations alongside operation counts), and quality (failure, cancellation, LLM fallback, paste failures and processing percentiles). Link the views and put metric definitions next to the charts. The project timezone determines day boundaries; incomplete days/weeks are not final results.

Count a distinct installation over the complete period; never sum daily unique counts. Version usage by event can count the same installation in several versions. For a non-overlapping adoption snapshot, select installations with a successful transcription in the observation window, take the version from each installation's last Sotto event in that window, then group those rows by version. This is the last observed version, not a live inventory. Explicitly label any fixed SQL window that does not follow dashboard date filters.

Failure rate is `failed / (completed + failed)`; cancellation rate is `cancelled / (completed + failed + cancelled)`. LLM fallback is measured among attempted LLM operations on completed transcriptions, and paste failure among completed microphone transcriptions. An empty denominator is no evidence of reliability, even if the chart renders it as zero. First observed successful use is not installation time; retention starts from that first observed success. Only observed telemetry-enabled installations are counted, and a person using two computers still counts twice.

Use these insight definitions across the three views:

1. DAU/WAU/MAU: unique `distinct_id` on `transcription.completed`.
2. Retention: return to `transcription.completed` (verify personless retention behavior in a staging project first).
3. Transcriptions per active installation and split by `pipeline_mode`, `source`, `stt_provider`, `stt_model`, `os`, `arch`, and `app_version`.
4. Sum and average of `audio_seconds` and `time_saved_seconds`.
5. Average, p50, and p95 `processing_seconds`.
6. Average session duration and actions per session from `usage_session.finished`.
7. Failure and cancellation rates, broken down by `stage` and `reason`.
8. LLM adoption (`llm_used`) and fallback rate (`llm_fallback`), split by `llm_provider` and `llm_model` where the model was validated by a response.
9. Interactive versus autostart launches and version adoption from `app.started`.

Use a separate PostHog project for development/staging payload inspection and production analytics. Restrict project access, enable MFA, and configure a spend limit or billing guard before putting the production ingest token into release builds.

Group model usage by `stt_model` for stable counts across display-name changes. For readable chart labels, use `stt_model_name` with `stt_model` as the fallback for older events, cloud models, and custom models. The display-name field is an optional addition to schema version 1.

Older `custom_local` events cannot be recovered: their original model IDs were discarded before delivery. Compare versions using `app_version` when assessing adoption after this change.

### Missing breakdown values (`None`)

The producer omits unavailable optional properties; it does not send a literal `None` provider or mode. Investigate an empty breakdown by checking the event type, the property's scope and `app_version` before treating it as lost telemetry. All Sotto dimensions below are **event properties**, not person properties; person profiles are disabled. PostHog documents these distinct [breakdown scopes](https://posthog.com/docs/product-analytics/trends/breakdowns).

| Question | Event and breakdown | Filter or interpretation |
| --- | --- | --- |
| Which transcription routes are used? | `transcription.completed` → `pipeline_mode` | Always populated in the current producer: `local`, `hybrid`, `cloud`, or `other`. |
| Which route dominates a usage session? | `usage_session.finished` → `dominant_pipeline_mode` | This event has no `pipeline_mode`; `app.started` has neither field. |
| Which STT adapters complete work? | `transcription.completed` → `stt_provider` | `local` or `compatible`; cloud failures and cancellations can omit the provider. |
| Which LLM providers are used successfully? | `transcription.completed` → `llm_provider` | Filter `llm_used = true` and provider is set. |
| Which LLM providers fall back? | `transcription.completed` → `llm_provider` | Filter `llm_fallback = true`; inspect missing providers separately. For a fallback rate, divide by relevant LLM operations, not all transcriptions. |
| Which validated LLM models are used? | `transcription.completed` → `llm_model` | Filter model is set; rejected or skipped requests intentionally omit it. |

Provider analytics describe operations, not saved settings. Local dictation without LLM processing has no `llm_provider` or `llm_model`, and an unrecognized provider is omitted by the allowlist. Cloud STT reports the fixed `compatible` adapter, not the service hostname or a custom profile name. Do not fill missing providers with the currently selected setting: that would report unused configurations as actual usage.

Keep the unfiltered completed-transcription series as the denominator for LLM adoption. Filtering all series to provider is set would silently remove non-LLM dictations and inflate adoption. For an overview that retains every completed transcription, a [SQL breakdown](https://posthog.com/docs/sql/expressions) can use `coalesce(properties.llm_provider, 'not_reported')`; label this as not reported, since absence alone does not prove that LLM processing was disabled. Check `llm_attempted`, `llm_used` and `llm_fallback` to distinguish outcomes.

If `pipeline_mode` or `stt_provider` is missing on current `transcription.completed` events, the normal producer contract does not explain it. Inspect those events and the chart's event/person property selection, filters and version range. Correcting a chart can reinterpret historical missing fields, but a new application build cannot recover values that were never sent.
