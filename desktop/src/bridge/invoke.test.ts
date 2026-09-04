// Vitest tests for `bridge/invoke.ts`.
//
// `invoke` is the central RPC entry point. In Tauri mode it's a
// direct passthrough to the native command; outside Tauri it
// throws a clear error (no HTTP bridge fallback — the Python
// sidecar is gone).

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const tauriMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => tauriMock(...args),
}));

beforeEach(() => {
  tauriMock.mockReset();
  delete (globalThis as { window?: unknown }).window;
  vi.resetModules();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("invoke() in Tauri mode (direct passthrough)", () => {
  beforeEach(() => {
    (globalThis as { window?: object }).window = {
      __TAURI_INTERNALS__: {},
    };
  });

  it("routes a call directly to the Tauri command", async () => {
    tauriMock.mockResolvedValueOnce({ ok: true });
    const { invoke } = await import("./invoke");
    const result = await invoke<{ ok: boolean }>("get_config");
    expect(result).toEqual({ ok: true });
    expect(tauriMock).toHaveBeenCalledWith("get_config", undefined);
  });

  it("passes params through unchanged", async () => {
    tauriMock.mockResolvedValueOnce("ok");
    const { invoke } = await import("./invoke");
    const params = { session_id: 7, microphone: "USB" };
    await invoke("start_microphone_test", params);
    expect(tauriMock).toHaveBeenCalledWith("start_microphone_test", params);
  });

  it("wraps the underlying error message", async () => {
    tauriMock.mockRejectedValueOnce(new Error("engine closed"));
    const { invoke } = await import("./invoke");
    await expect(invoke("get_config")).rejects.toThrow("engine closed");
  });

  it("converts non-Error rejections into Error instances", async () => {
    tauriMock.mockRejectedValueOnce("string thrown");
    const { invoke } = await import("./invoke");
    await expect(invoke("get_config")).rejects.toThrow("string thrown");
  });
});

describe("invoke() outside Tauri (clear error, no HTTP bridge)", () => {
  beforeEach(() => {
    // Ensure window.__TAURI_INTERNALS__ is NOT set
    delete (globalThis as { window?: unknown }).window;
  });

  it("throws an error saying Tauri runtime is required", async () => {
    const { invoke } = await import("./invoke");
    await expect(invoke("get_config")).rejects.toThrow(/requires Tauri runtime/);
  });

  it("mentions the method name in the error", async () => {
    const { invoke } = await import("./invoke");
    await expect(invoke("some_method")).rejects.toThrow(/some_method/);
  });

  it("does not attempt an HTTP fetch", async () => {
    globalThis.fetch = vi.fn();
    const { invoke } = await import("./invoke");
    await expect(invoke("get_config")).rejects.toThrow();
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });
});
