import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { subscribe } from "./events";

const native = vi.hoisted(() => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }));

beforeEach(() => {
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  native.listen.mockReset();
});
afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

describe("component event lifetime", () => {
  it("blocks late events and releases a registration that finishes after cleanup", async () => {
    let complete!: (stop: () => void) => void;
    let emit!: (event: { payload: number }) => void;
    native.listen.mockImplementation((_name, callback) => {
      emit = callback;
      return new Promise<() => void>((resolve) => { complete = resolve; });
    });
    const received: number[] = [];
    const dispose = subscribe<number>("history-updated", (value) => received.push(value));
    emit({ payload: 1 });
    dispose();
    emit({ payload: 2 });
    const stop = vi.fn();
    complete(stop);
    await vi.waitFor(() => expect(stop).toHaveBeenCalledOnce());
    dispose();
    expect(stop).toHaveBeenCalledOnce();
    expect(received).toEqual([1]);
  });

  it("unsubscribes an already registered listener and tolerates repeated cleanup", async () => {
    const stop = vi.fn();
    native.listen.mockResolvedValue(stop);
    const dispose = subscribe("config-updated", () => {});
    await new Promise((resolve) => setTimeout(resolve, 0));
    dispose();
    dispose();
    expect(stop).toHaveBeenCalledOnce();
  });

  it("handles registration failure without an unhandled rejection", async () => {
    native.listen.mockRejectedValue(new Error("event transport unavailable"));
    const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
    const dispose = subscribe("config-updated", () => {});
    await vi.waitFor(() => expect(warning).toHaveBeenCalledOnce());
    expect(dispose).not.toThrow();
  });
});
