// Vitest tests for the session filtering in `bridge/recording.ts`.
//
// The bridge follows one dictation at a time and has to tell a stale event
// apart from an event for the session it is tracking. Getting that wrong is
// not cosmetic: every terminal event is what moves the shared recording state
// off "processing", so a dropped one leaves the tray and the status bar
// claiming work that finished long ago.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Handler = (payload: unknown) => void;

const handlers = new Map<string, Handler[]>();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: async (event: string, cb: (e: { payload: unknown }) => void) => {
    handlers.set(event, [...(handlers.get(event) ?? []), (payload) => cb({ payload })]);
    return () => {};
  },
}));

function emit(event: string, payload: unknown) {
  for (const handler of handlers.get(event) ?? []) handler(payload);
}

// `on()` resolves through the mocked `listen`, so the subscriptions are only
// in place after the microtask queue drains.
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

async function subscribe() {
  const mod = await import("./recording");
  mod.onRecordingStateChange(() => {});
  await flush();
  return mod;
}

beforeEach(() => {
  handlers.clear();
  vi.resetModules();
  (globalThis as { window?: object }).window = {
    __TAURI_INTERNALS__: {},
    setTimeout: (fn: () => void, ms: number) => setTimeout(fn, ms),
    clearTimeout: (id: unknown) => clearTimeout(id as never),
  };
});

afterEach(() => {
  delete (globalThis as { window?: unknown }).window;
  vi.restoreAllMocks();
});

describe("recording bridge session routing", () => {
  it("accepts a scoped event when no session is being tracked", async () => {
    // A window opened midway through a dictation never saw `recording-started`
    // and so has no id to compare against. There is no newer session to
    // protect here, and refusing the event strands the UI in "processing".
    const mod = await subscribe();

    emit("recording-stopped", 7);
    expect(mod.getRecordingState()).toBe("processing");

    emit("paste-done", { session_id: 7, length: 12 });
    expect(mod.getRecordingState()).toBe("done");
  });

  it("ignores an event from a session other than the tracked one", async () => {
    const mod = await subscribe();

    emit("recording-started", 8);
    expect(mod.getRecordingState()).toBe("recording");

    emit("paste-done", { session_id: 7, length: 12 });
    expect(mod.getRecordingState()).toBe("recording");
    expect(mod.getCurrentSessionId()).toBe(8);
  });

  it("advances the state and releases the id on the same terminal event", async () => {
    // Both halves are the contract of one event. They used to live in two
    // separate `listen` registrations, whose invocation order nothing
    // guarantees — and in the losing order the reset ran first and the state
    // transition was then discarded as stale.
    const mod = await subscribe();

    emit("recording-started", 8);
    emit("whisper-done", { session_id: 8, text: "привет" });
    expect(mod.getRecordingState()).toBe("processing");
    // Still cancellable: the LLM pass runs after `whisper-done`.
    expect(mod.getCurrentSessionId()).toBe(8);

    emit("paste-done", { session_id: 8, length: 6 });
    expect(mod.getRecordingState()).toBe("done");
    expect(mod.getCurrentSessionId()).toBeNull();
  });

  it("preserves the active cycle when another start is refused", async () => {
    const mod = await subscribe();

    emit("recording-started", 8);
    emit("whisper-failed", { message: "Идёт транскрипция файла" });

    expect(mod.getRecordingState()).toBe("recording");
    expect(mod.getCurrentSessionId()).toBe(8);
  });

  it("keeps recording through model events and finishes after a scoped error", async () => {
    const mod = await subscribe();
    emit("recording-started", 8);
    emit("whisper-loading", "tiny");
    emit("whisper-ready", "tiny");
    expect(mod.getRecordingState()).toBe("recording");
    emit("whisper-failed", { session_id: 8, message: "capture failed" });
    expect(mod.getRecordingState()).toBe("error");
    expect(mod.getCurrentSessionId()).toBeNull();
    emit("recording-started", 9);
    emit("whisper-cancelled", 8);
    emit("recording-stopped", 9);
    emit("paste-done", { session_id: 9 });
    expect(mod.getRecordingState()).toBe("done");
  });

  it("leaves model loading when the Rust model becomes ready", async () => {
    const mod = await subscribe();
    emit("whisper-loading", "tiny");
    expect(mod.getRecordingState()).toBe("loading");
    emit("whisper-ready", "tiny");
    expect(mod.getRecordingState()).toBe("idle");
  });
});
