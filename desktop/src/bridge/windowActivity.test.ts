import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { isWindowShown, subscribeWindowActivity } from "./windowActivity";

const native = vi.hoisted(() => ({
  isVisible: vi.fn(), isMinimized: vi.fn(), isFocused: vi.fn(), onFocusChanged: vi.fn(),
}));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => native }));

let onFocus: (event: { payload: boolean }) => void;
let stop = vi.fn<() => void>();
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  vi.resetAllMocks();
  vi.stubGlobal("window", new EventTarget());
  vi.stubGlobal("document", Object.assign(new EventTarget(), { visibilityState: "visible" }));
  native.isVisible.mockResolvedValue(true);
  native.isMinimized.mockResolvedValue(false);
  native.isFocused.mockResolvedValue(true);
  stop = vi.fn<() => void>();
  native.onFocusChanged.mockImplementation((handler) => { onFocus = handler; return Promise.resolve(stop); });
});
afterEach(() => { vi.unstubAllGlobals(); });

describe("native window visibility", () => {
  it.each([
    [true, false, true], [false, false, false], [true, true, false],
  ])("counts a visible, unminimized window as shown (%s, %s)", async (visible, minimized, shown) => {
    native.isVisible.mockResolvedValue(visible);
    native.isMinimized.mockResolvedValue(minimized);
    await expect(isWindowShown()).resolves.toBe(shown);
  });

  it("assumes a shown window when the native query fails", async () => {
    native.isVisible.mockRejectedValue(new Error("gone"));
    await expect(isWindowShown()).resolves.toBe(true);
  });
});

describe("native window activity", () => {
  it.each([
    [false, false, true], [true, true, true], [true, false, false],
  ])("does not consider hidden, minimized, or unfocused windows active (%s, %s, %s)", async (visible, minimized, focused) => {
    native.isVisible.mockResolvedValue(visible);
    native.isMinimized.mockResolvedValue(minimized);
    native.isFocused.mockResolvedValue(focused);
    const active = vi.fn();
    const dispose = subscribeWindowActivity(active);
    await flush();
    expect(active).toHaveBeenLastCalledWith(false);
    dispose();
  });

  it("rejects a stale visibility response after a native blur", async () => {
    let resolveVisible!: (value: boolean) => void;
    native.isVisible.mockReturnValueOnce(new Promise<boolean>((resolve) => { resolveVisible = resolve; }));
    const active = vi.fn();
    const dispose = subscribeWindowActivity(active);
    onFocus({ payload: false });
    resolveVisible(true);
    await flush();
    expect(active.mock.calls).toEqual([[false]]);
    onFocus({ payload: true });
    await flush();
    expect(active).toHaveBeenLastCalledWith(true);
    dispose();
  });

  it("honors document visibility and recovers on reopening", async () => {
    Object.assign(document, { visibilityState: "hidden" });
    const active = vi.fn();
    const dispose = subscribeWindowActivity(active);
    await flush();
    expect(active).toHaveBeenLastCalledWith(false);
    expect(native.isVisible).not.toHaveBeenCalled();
    Object.assign(document, { visibilityState: "visible" });
    document.dispatchEvent(new Event("visibilitychange"));
    await flush();
    expect(active).toHaveBeenLastCalledWith(true);
    dispose();
  });

  it("stops a listener whose registration completes after disposal", async () => {
    let resolveListener!: (value: () => void) => void;
    native.onFocusChanged.mockImplementation((handler) => {
      onFocus = handler;
      return new Promise<() => void>((resolve) => { resolveListener = resolve; });
    });
    const active = vi.fn();
    const dispose = subscribeWindowActivity(active);
    dispose();
    resolveListener(stop);
    await flush();
    onFocus({ payload: true });
    expect(stop).toHaveBeenCalledOnce();
    expect(active).not.toHaveBeenCalled();
  });

  it("keeps a failed native visibility query inactive", async () => {
    native.isVisible.mockRejectedValueOnce(new Error("Synthetic query failure"));
    const active = vi.fn();
    const dispose = subscribeWindowActivity(active);
    await flush();
    expect(active).toHaveBeenLastCalledWith(false);
    dispose();
  });
});
