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

  it("paints the first frame even when it arrives right after subscribing", async () => {
    const { onFrame } = await import("./frameClock");
    clock = 10;
    const seconds: number[] = [];
    const stop = onFrame((_now, delta) => { seconds.push(delta); });
    // Browsers time a frame by its start, which can precede the request made during it.
    const [frame] = queued;
    queued = [];
    frame(8);
    refresh(1, 5);
    refresh(1, 1000 / 60);
    stop();
    expect(seconds[0]).toBe(0);
    expect(seconds).toHaveLength(2);
  });
});

describe("frameRunner", () => {
  it.each([1000 / 60, 250, 800])("settles the last reading before sleeping with %s ms frames", async (refreshMs) => {
    const { easeStep, frameRunner } = await import("./frameClock");
    let level = 1;
    const runner = frameRunner((_now, seconds) => {
      level += (0 - level) * easeStep(seconds, 0.035);
      if (level < 0.001) level = 0;
    });
    runner.wake();
    refresh(Math.ceil(800 / refreshMs), refreshMs);
    expect(level).toBe(0);
    expect(queued).toHaveLength(0);
  });

  it("excludes time spent asleep when readings resume", async () => {
    const { frameRunner } = await import("./frameClock");
    const seconds: number[] = [];
    const runner = frameRunner((_now, delta) => { seconds.push(delta); });
    runner.wake();
    refresh(1, 800);
    refresh(1, 10_000);
    runner.wake();
    refresh(1, 1000 / 60);
    expect(seconds).toHaveLength(2);
    expect(seconds[1]).toBeCloseTo(1 / 60);
    runner.stop();
  });

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
