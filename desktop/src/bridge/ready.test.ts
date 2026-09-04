// Vitest tests for `bridge/ready.ts`.
//
// `waitForReady` is a no-op in Tauri mode (the Rust app is
// always ready when the frontend loads) and a polling probe in
// the dev-only HTTP-bridge fallback. Both branches must remain
// pinned.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

beforeEach(() => {
  delete (globalThis as { window?: unknown }).window;
  vi.resetModules();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("waitForReady()", () => {
  it("returns immediately in Tauri mode (no polling)", async () => {
    (globalThis as { window?: object }).window = {
      __TAURI_INTERNALS__: {},
    };
    const fetchSpy = vi.fn();
    globalThis.fetch = fetchSpy;
    const { waitForReady } = await import("./ready");
    await waitForReady();
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("polls the HTTP bridge until /ready returns 2xx", async () => {
    delete (globalThis as { window?: unknown }).window;
    const fetchMock = vi
      .fn()
      // First two attempts fail with a network error,
      // third returns 200.
      .mockRejectedValueOnce(new Error("ECONNREFUSED"))
      .mockRejectedValueOnce(new Error("ECONNREFUSED"))
      .mockResolvedValueOnce({ ok: true });
    globalThis.fetch = fetchMock;
    const { waitForReady } = await import("./ready");
    const promise = waitForReady(2_000);
    // Drive the polling loop manually — fake timers sleep real
    // ms, and the bridge back-off uses `setTimeout(_, 200)`.
    // We advance a few cycles and let the 3rd attempt resolve.
    await vi.runAllTimersAsync();
    await promise;
    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:9137/ready",
    );
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it("throws once the deadline expires", async () => {
    delete (globalThis as { window?: unknown }).window;
    globalThis.fetch = vi.fn().mockRejectedValue(new Error("never up"));
    const { waitForReady } = await import("./ready");
    const promise = waitForReady(500);
    const caught = expect(promise).rejects.toThrow(/within timeout/);
    await vi.runAllTimersAsync();
    await caught;
  });
});