# Architecture overview

Sotto is a Tauri desktop application with a React/TypeScript frontend and a
Rust backend. The frontend invokes a deliberately small command/event bridge;
the Rust side owns audio capture, local model execution, cloud adapters,
clipboard/paste integration, settings, history, statistics, and telemetry.

The default path is:

```text
global hotkey → audio capture → local STT → optional text formatting → paste/copy
```

File transcription uses the same speech pipeline without touching the focused
window or adding the result to history. Local model files are downloaded into
the application cache; see [Models](models.md) for the current engine split and
platform restrictions.

The frontend is built as three separate pages, one per window: `index.html`
(settings), `overlay.html` (the recording pill), and `tray.html` (the tray
popup). Rust opens each window at its own URL, so the overlay does not download
the settings UI to draw one line of text. Shared code — the bridge, i18n,
React — is extracted into chunks the pages have in common.

This is intentionally a boundary-level document: it describes the boundaries
that hold today, not the route taken to them.
