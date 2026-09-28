import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// A display that refreshes every `refreshMs`, driven by hand.
let queued: Array<(now: number) => void> = [];
let clock = 0;
function refresh(frames: number, refreshMs: number) {
  for (let frame = 0; frame < frames; frame++) {
    clock += refreshMs;
    const due = queued;
    queued = [];
    due.forEach((callback) => callback(clock));
  }
}

beforeEach(() => {
  queued = []; clock = 0;
  vi.stubGlobal("requestAnimationFrame", (callback: (now: number) => void) => { queued.push(callback); return queued.length; });
  vi.stubGlobal("cancelAnimationFrame", () => { queued = []; });
  vi.spyOn(performance, "now").mockImplementation(() => clock);
  vi.resetModules();
});
afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

describe("onFrame", () => {
  it("holds a 144 Hz display to about 60 frames a second", async () => {
    const { onFrame } = await import("./frameClock");
    let painted = 0;
    const stop = onFrame(() => { painted++; });
    refresh(144, 1000 / 144);
    stop();
    expect(painted).toBeGreaterThanOrEqual(55);
    expect(painted).toBeLessThanOrEqual(73);
  });

  it("keeps every frame of a 60 Hz display", async () => {
    const { onFrame } = await import("./frameClock");
    let painted = 0;
    const stop = onFrame(() => { painted++; });
    refresh(60, 1000 / 60);
    stop();
    expect(painted).toBe(60);
  });
});

describe("frameRunner", () => {
  it("stops painting once readings stop coming", async () => {
    const { frameRunner } = await import("./frameClock");
    let painted = 0;
    const runner = frameRunner(() => { painted++; }, 200);
    runner.wake();
    refresh(60, 1000 / 60);
    const afterSecond = painted;
    refresh(60, 1000 / 60);
    expect(afterSecond).toBeLessThan(20);
    expect(painted).toBe(afterSecond);
  });
});
