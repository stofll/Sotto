import { describe, expect, it } from "vitest";
import { DISSOLVE_MS, dissolveShows, dissolveStep, dissolveThreshold } from "./shellDissolve";

describe("dissolve mask", () => {
  const cells: Array<[number, number]> = [];
  for (let y = 0; y < 20; y++) for (let x = 0; x < 100; x++) cells.push([x, y]);
  const shown = (progress: number) => cells.filter(([x, y]) => dissolveShows(x, y, progress)).length;

  it("starts whole and ends gone", () => {
    expect(shown(0)).toBe(cells.length);
    expect(shown(1)).toBe(0);
  });

  it("only removes cells: nothing that is gone comes back", () => {
    let before = cells.filter(([x, y]) => dissolveShows(x, y, 0.2));
    for (const progress of [0.4, 0.6, 0.8, 1]) {
      const now = cells.filter(([x, y]) => dissolveShows(x, y, progress));
      expect(now.every((cell) => before.includes(cell))).toBe(true);
      expect(now.length).toBeLessThanOrEqual(before.length);
      before = now;
    }
  });

  it("is spread out, not a sweep: halfway, about half the cells are left on both sides", () => {
    const half = cells.filter(([x, y]) => dissolveShows(x, y, 0.5));
    const left = half.filter(([x]) => x < 50).length, right = half.length - left;
    expect(half.length / cells.length).toBeGreaterThan(0.35);
    expect(half.length / cells.length).toBeLessThan(0.65);
    expect(Math.abs(left - right) / half.length).toBeLessThan(0.15);
  });

  it("is the same every time, so a frame can be cached", () => {
    expect(dissolveThreshold(17, 5)).toBe(dissolveThreshold(17, 5));
    for (const [x, y] of cells) {
      expect(dissolveThreshold(x, y)).toBeGreaterThanOrEqual(0);
      expect(dissolveThreshold(x, y)).toBeLessThan(1);
    }
  });
});

describe("dissolve timing", () => {
  it("fits inside the time the native side leaves the window up", () => {
    // LEAVE_DURATION in overlay.rs is 220 ms; the dissolve must end well inside it.
    expect(DISSOLVE_MS).toBeLessThanOrEqual(180);
  });

  it("steps from the first mask to the last and then holds the last", () => {
    expect(dissolveStep(0)).toBe(0);
    expect(dissolveStep(DISSOLVE_MS / 2)).toBeGreaterThan(0);
    expect(dissolveStep(DISSOLVE_MS)).toBe(dissolveStep(DISSOLVE_MS * 5));
    expect(dissolveStep(-30)).toBe(0);
    let last = 0;
    for (let elapsed = 0; elapsed <= DISSOLVE_MS; elapsed += 5) {
      expect(dissolveStep(elapsed)).toBeGreaterThanOrEqual(last);
      last = dissolveStep(elapsed);
    }
  });
});
