# Architecture overview

Sotto is a Tauri desktop application with a React/TypeScript frontend and a Rust backend. The frontend invokes a deliberately small command/event bridge; the Rust side owns audio capture, local model execution, cloud adapters, clipboard/paste integration, settings, history, statistics, and telemetry.

The default path is:

```text
global hotkey → audio capture → local STT → optional text formatting → paste/copy
```

File transcription uses the same speech pipeline without touching the focused window or adding the result to history.

Local model files are downloaded into the application cache; see [Models](models.md) for the current engine split and platform restrictions.

The frontend has three pages, one per window:

- `index.html` — settings.
- `overlay.html` — the recording overlay.
- `tray.html` — the tray popup.

Rust opens each window at its own URL, so the overlay does not download the settings UI. Shared bridge, i18n, and React code is extracted into common chunks.

This is intentionally a boundary-level document: it describes the boundaries that hold today, not the route taken to them.

## Overlay presentation and event lifetime

Keep overlay session transitions and cancellation in `useOverlaySession`, and visual layout in the overlay components and stylesheet. Appearance preferences come from configuration; a pure palette function derives the overlay colors. Native geometry changes run through the same worker queue as show/hide.

Audio levels belong to the waveform component so frequent samples do not rerender the whole window; time updates run only while recording or waiting for post-processing. New visual variants must preserve the session contract and fit the native window geometry.

The bead hides streaming text and expands only for errors or LLM fallback warnings; its cancel button appears on hover. The glow keeps streaming text and errors in the composer shape and drives its wash from the same `audio-level` events as the waveform. See [Overlay appearance](overlay.md) for user settings.

Component-owned event subscriptions use `bridge/events.subscribe`, whose synchronous cleanup also disposes registrations that finish after unmount. Use the asynchronous `on` only when an operation must await registration before starting work, or when the subscription deliberately lives for the entire webview lifetime.
