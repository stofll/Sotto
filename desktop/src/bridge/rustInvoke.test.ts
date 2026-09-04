// Vitest tests for `bridge/rustInvoke.ts`.
//
// `rustInvoke` is the strict variant of `invoke` that ONLY works
// in Tauri mode. Outside Tauri it must throw — both `rustInvoke`
// and `invoke` now reject with a clear error (no HTTP bridge fallback).

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

describe("rustInvoke()", () => {
  it("routes directly to the Tauri command when Tauri is present", async () => {
    (globalThis as { window?: object }).window = {
      __TAURI_INTERNALS__: {},
    };
    tauriMock.mockResolvedValueOnce("ok");
    const { rustInvoke } = await import("./rustInvoke");
    const result = await rustInvoke<string>("start_recording");
    expect(result).toBe("ok");
    expect(tauriMock).toHaveBeenCalledWith("start_recording", undefined);
  });

  it("throws when called outside the Tauri runtime", async () => {
    delete (globalThis as { window?: unknown }).window;
    const { rustInvoke } = await import("./rustInvoke");
    await expect(rustInvoke("start_recording")).rejects.toThrow(
      /requires Tauri runtime/,
    );
  });

  it("forwards Error rejections verbatim", async () => {
    (globalThis as { window?: object }).window = {
      __TAURI_INTERNALS__: {},
    };
    tauriMock.mockRejectedValueOnce(new Error("cpal: device busy"));
    const { rustInvoke } = await import("./rustInvoke");
    await expect(rustInvoke("start_recording")).rejects.toThrow(
      "cpal: device busy",
    );
  });
});