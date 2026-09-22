import { describe, expect, it } from "vitest";
import { timingChartPath } from "./timingChart";

function segments(path: string) {
  const [start, ...curves] = path.split(" C");
  const startPoint = start.slice(1).split(",").map(Number);
  let previous = startPoint;
  return curves.map((curve) => {
    const points = curve.split(" ").map((point) => point.split(",").map(Number));
    const segment = [previous, ...points];
    previous = points[2];
    return segment;
  });
}

describe("daily timing curve", () => {
  it.each([
    [0, 0, 80, 0, 0, 0, 20, 0],
    [0, 1, 40, 41, 80, 79, 2, 0],
    [80, 40, 20, 10, 5, 0],
    [0, 0, 0, 0],
    [5, 5, 5, 5],
  ])("preserves daily values and stays between adjacent observations: %j", (...values) => {
    const max = Math.max(0.01, ...values);
    const curves = segments(timingChartPath(values, max));
    expect(curves).toHaveLength(values.length - 1);
    curves.forEach(([p0, p1, p2, p3], i) => {
      expect(p0[0]).toBeCloseTo(i / (values.length - 1) * 100, 3);
      expect(p3[0]).toBeCloseTo((i + 1) / (values.length - 1) * 100, 3);
      expect(p0[1]).toBeCloseTo(100 - values[i] / max * 90, 3);
      expect(p3[1]).toBeCloseTo(100 - values[i + 1] / max * 90, 3);
      for (let step = 0; step <= 100; step++) {
        const t = step / 100, u = 1 - t;
        const y = u ** 3 * p0[1] + 3 * u ** 2 * t * p1[1] + 3 * u * t ** 2 * p2[1] + t ** 3 * p3[1];
        expect(y).toBeGreaterThanOrEqual(Math.min(p0[1], p3[1]) - 1e-6);
        expect(y).toBeLessThanOrEqual(Math.max(p0[1], p3[1]) + 1e-6);
      }
    });
  });

  it("handles empty, single-day and two-day series without invalid geometry", () => {
    expect(timingChartPath([], 0)).toBe("");
    expect(timingChartPath([0], 0)).toBe("M0.0000,100.0000");
    expect(segments(timingChartPath([0, 10], 10))).toEqual([
      [[0, 100], [33.3333, 70], [66.6667, 40], [100, 10]],
    ]);
  });
});
