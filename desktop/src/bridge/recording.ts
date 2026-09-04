import { rustInvoke } from "./rustInvoke";
import { on } from "./events";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { sessionIdOf } from "./sessionEvents";

export type RecordingState = "idle" | "recording" | "processing" | "done" | "error" | "loading";

let _state: RecordingState = "idle";
let _stateListeners: Array<(state: RecordingState) => void> = [];
let _subscribed = false;
let _unlisteners: UnlistenFn[] = [];
let _settledStateTimer: ReturnType<typeof window.setTimeout> | null = null;
let _currentSessionId: number | null = null;

export function getRecordingState(): RecordingState {
  return _state;
}

export function getCurrentSessionId(): number | null {
  return _currentSessionId;
}

export function onRecordingStateChange(cb: (state: RecordingState) => void): () => void {
  _stateListeners.push(cb);
  ensureSubscribed();
  return () => { _stateListeners = _stateListeners.filter((l) => l !== cb); };
}

function setState(next: RecordingState) {
  if (_state === next) return;
  if (_settledStateTimer !== null) {
    window.clearTimeout(_settledStateTimer);
    _settledStateTimer = null;
  }
  _state = next;
  // Overlay is driven by Rust (sidecar reader → sync_overlay). The bridge
  // only tracks state for React consumers (status bar, tray badge, etc.) —
  // don't invoke show_state/hide from here, that would duplicate native
  // calls and spawn the overlay window for non-recording events.
  for (const cb of _stateListeners) cb(next);

  if (next === "done" || next === "error") {
    const delay = next === "done" ? 2200 : 4500;
    _settledStateTimer = window.setTimeout(() => {
      _settledStateTimer = null;
      setState("idle");
    }, delay);
  }
}

function ensureSubscribed() {
  if (_subscribed) return;
  _subscribed = true;

  const isRustActive = () => _state === "recording" || _state === "processing";

  const flatEvents: Record<string, RecordingState> = {
    // Rust events (recording-flow, WS 4a2b).
    "recording-started": "recording",
    "recording-stopped": "processing",
    "whisper-started": "processing",
    // Decoding is done, the cycle is not: local formatting and the LLM
    // pass still run, and only then is the text pasted. Settling to "done"
    // here would drop the status back to idle while the LLM is still
    // working. `paste-done` is the real finish line.
    "whisper-done": "processing",
    "paste-done": "done",
    // Empty transcription (silence / too-short audio): the Rust dispatcher
    // hides the overlay and emits `whisper-empty` instead of `whisper-done`.
    // Return the UI to idle — without this handler the tray stays stuck on
    // "Распознаю" forever after an empty result.
    "whisper-empty": "idle",
    "whisper-cancelled": "idle",
    "whisper-failed": "error",
    "paste-failed": "error",
    // Python events (model-loading, until WS 4c) — gated by isRustActive().
    "whisper-loading": "loading",
    "whisper-load-failed": "error",
  };

  const sessionEvents = new Set([
    "recording-stopped",
    "whisper-started",
    "whisper-done",
    "paste-done",
    "paste-failed",
    "whisper-empty",
    "whisper-cancelled",
    "whisper-failed",
  ]);
  // Events after which the id must not be handed to `cancelRecording()` any
  // more. `recording-stopped` and `whisper-done` are deliberately absent:
  // both are intermediate states where the overlay can still cancel while
  // the LLM pass runs.
  const terminalEvents = new Set([
    "paste-done",
    "paste-failed",
    "whisper-empty",
    "whisper-cancelled",
    "whisper-failed",
  ]);
  // One listener per event, doing both the state transition and the session
  // bookkeeping. Two `listen` registrations for the same event are two
  // independent async IPC calls, and nothing guarantees which of them the
  // backend invokes first — splitting these let a reset run before the
  // transition that reads it, which stuck the UI in "processing".
  for (const [ev, next] of Object.entries(flatEvents)) {
    on<unknown>(ev, (payload) => {
      const sid = sessionIdOf(payload);
      if (ev === "recording-started" && sid !== null) _currentSessionId = sid;
      // Drop only genuinely stale events: a scoped event for a session other
      // than the one being tracked. With no tracked session there is nothing
      // newer to protect — this window may have subscribed midway through a
      // dictation and never saw its `recording-started` — so the event still
      // counts. Startup/refusal errors carry no session id and always pass.
      if (sessionEvents.has(ev) && sid !== null && _currentSessionId !== null && sid !== _currentSessionId) return;
      if (terminalEvents.has(ev) && sid !== null) _currentSessionId = null;
      setState(next);
    }).then((fn) => _unlisteners.push(fn));
  }

  // Conditional handlers (Python model-loaded/unloaded with precedence guard).
  on<unknown>("model-ready", () => {
    if (_state === "loading" && !isRustActive()) setState("idle");
  }).then((fn) => _unlisteners.push(fn));
  on<unknown>("model-unloaded", () => {
    if (_state === "loading" && !isRustActive()) setState("idle");
  }).then((fn) => _unlisteners.push(fn));

  // Hotkey error surface.
  on<string>("hotkey-error", (msg) => {
    if (!isRustActive()) setState("error");
    console.warn("hotkey error:", msg);
  }).then((fn) => _unlisteners.push(fn));
}

export async function startRecording(): Promise<number> {
  return await rustInvoke<number>("start_recording");
}

export async function stopRecording(): Promise<number> {
  return await rustInvoke<number>("stop_recording");
}

export async function cancelRecording(): Promise<boolean> {
  const sessionId = _currentSessionId;
  if (sessionId === null) {
    console.warn("cancelRecording: no active session");
    return false;
  }
  return rustInvoke<boolean>("cancel_recording", { sessionId });
}
