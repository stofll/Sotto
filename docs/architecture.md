# Architecture overview

Sotto is a Tauri desktop application with a React/TypeScript frontend and a Rust backend. The frontend invokes a deliberately small command/event bridge; the Rust side owns audio capture, local model execution, cloud adapters, clipboard/paste integration, settings, history, statistics, and telemetry.

The default path is:

```text
global hotkey → audio capture → local STT → optional text formatting → paste/copy
```

File transcription uses the same speech pipeline without touching the focused window or adding the result to history. Cancellation is available from file decoding through optional LLM processing, and leaving the file panel cancels its active session.

Decoding checks cancellation between packets; an in-progress blocking file read or codec call must finish before the engine claim is released. Once speech recognition finishes, other dictations can use the engine independently.

Local model files are downloaded into the application cache; see [Models](models.md) for the current engine split and platform restrictions.

The frontend has two pages, one per window:

- `index.html` — settings.
- `overlay.html` — the recording overlay.

Rust opens each window at its own URL, so the overlay does not download the settings UI. The system tray uses a native menu. Shared bridge, i18n, and React code is extracted into common chunks.

This is intentionally a boundary-level document: it describes the boundaries that hold today, not the route taken to them.

## Overlay presentation and event lifetime

Keep overlay session transitions and cancellation in `useOverlaySession`, and visual layout in the overlay components and stylesheet. Appearance preferences come from configuration; a pure palette function derives the overlay colors. Native geometry changes run through the same worker queue as show/hide.

A hide does not conceal the window at once. The worker emits `overlay-leaving`, the WebView plays its exit, and the window is concealed after the fixed `LEAVE_DURATION` in `overlay.rs`, so every exit animation must end within it. A show that arrives during the exit cancels the conceal and keeps the window on screen; a new dictation never waits for the previous exit.

Audio levels belong to the waveform component so frequent samples do not rerender the whole window; time updates run only while recording or waiting for post-processing. New visual variants must preserve the session contract and fit the native window geometry.

The bead hides streaming text and expands only for errors or LLM fallback warnings. The glow keeps streaming text and errors in the composer shape and drives its beam from the same `audio-level` events as the waveform. Every shape shows its cancel control on hover or keyboard focus. The timer can be turned off in Overlay settings. See [Overlay appearance](overlay.md) for user settings.

Component-owned event subscriptions use `bridge/events.subscribe`, whose synchronous cleanup also disposes registrations that finish after unmount. Use the asynchronous `on` only when an operation must await registration before starting work, or when the subscription deliberately lives for the entire webview lifetime.
