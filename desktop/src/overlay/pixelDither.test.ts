import { describe, expect, it } from "vitest";
import { errorDiffusion, historyAt, inkForFill, orderedDither, oklchToRgb, smoothstep, toneColour, toneTable } from "./pixelDither";

describe("orderedDither", () => {
  const share = (intensity: number, steps: number) => {
    let total = 0;
    for (let y = 0; y < 4; y++) for (let x = 0; x < 4; x++) total += orderedDither(intensity, x, y, steps);
    return total / 16 / steps;
  };

  it("stays empty at 0 and full at 1, and clamps what lies outside", () => {
    expect(share(0, 4)).toBe(0);
    expect(share(1, 4)).toBe(1);
    expect(share(-3, 4)).toBe(0);
    expect(share(7, 4)).toBe(1);
  });

  it("averages to the intensity over one Bayer tile", () => {
    for (const intensity of [0.125, 0.3, 0.5, 0.8]) expect(share(intensity, 4)).toBeCloseTo(intensity, 1);
  });

  it("never lights more tones than asked for", () => {
    for (let y = 0; y < 4; y++) for (let x = 0; x < 4; x++) expect(orderedDither(0.99, x, y, 3)).toBeLessThanOrEqual(3);
  });
});

describe("errorDiffusion", () => {
  const run = (value: number, width = 24, height = 12) => {
    const out = new Uint8Array(width * height);
    errorDiffusion(new Float32Array(width * height).fill(value), width, height, out, new Float32Array((width + 2) * (height + 1)));
    return out.reduce((sum, on) => sum + on, 0) / out.length;
  };

  it("lights nothing for black and everything for white", () => {
    expect(run(0)).toBe(0);
    expect(run(1)).toBe(1);
  });

  it("keeps the average tone of a flat grey", () => {
    for (const value of [0.25, 0.5, 0.75]) expect(run(value)).toBeCloseTo(value, 1);
  });

  it("does not carry error from one run into the next", () => {
    const width = 8, height = 4, scratch = new Float32Array((width + 2) * (height + 1)), out = new Uint8Array(width * height);
    errorDiffusion(new Float32Array(width * height).fill(0.4), width, height, out, scratch);
    errorDiffusion(new Float32Array(width * height), width, height, out, scratch);
    expect(out.every((on) => on === 0)).toBe(true);
  });
});

describe("historyAt", () => {
  it("counts back from the newest reading and repeats the oldest beyond the history", () => {
    const levels = [0.1, 0.2, 0.3];
    expect(historyAt(levels, 0)).toBe(0.3);
    expect(historyAt(levels, 1.4)).toBe(0.2);
    expect(historyAt(levels, 2)).toBe(0.1);
    expect(historyAt(levels, 50)).toBe(0.1);
    expect(historyAt([], 3)).toBe(0);
  });
});

describe("oklchToRgb", () => {
  it("maps the lightness axis to grey and clamps out-of-gamut colours", () => {
    expect(oklchToRgb(1, 0, 0)).toEqual([255, 255, 255]);
    expect(oklchToRgb(0, 0, 0)).toEqual([0, 0, 0]);
    const [r, g, b] = oklchToRgb(0.6, 0.5, 30);
    for (const channel of [r, g, b]) expect(channel).toBeGreaterThanOrEqual(0), expect(channel).toBeLessThanOrEqual(255);
  });
});

describe("toneTable", () => {
  const luma = (table: Uint8ClampedArray, x: number) => { const i = Math.round(x * 255) * 3; return table[i] + table[i + 1] + table[i + 2]; };

  it("brightens with the tone for light ink and darkens for dark ink", () => {
    const light = toneTable(55, 0.14), dark = toneTable(55, 0.14, "dark");
    expect(luma(light, 1)).toBeGreaterThan(luma(light, 0));
    expect(luma(dark, 1)).toBeLessThan(luma(dark, 0));
  });

  it("gives a CSS colour from a tone", () => {
    expect(toneColour(toneTable(0, 0), 1, 0.5)).toMatch(/^rgba\(\d+,\d+,\d+,0\.5\)$/);
  });
});

describe("smoothstep", () => {
  it("eases between its edges", () => {
    expect(smoothstep(0, 1, -1)).toBe(0);
    expect(smoothstep(0, 1, 0.5)).toBe(0.5);
    expect(smoothstep(0, 1, 2)).toBe(1);
  });
});

describe("inkForFill", () => {
  it("draws dark marks on the light fills only", () => {
    expect(["light", "paper"].map(inkForFill)).toEqual(["dark", "dark"]);
    expect(["palette", "black", "none", "smoke", null, undefined].map(inkForFill)).toEqual(Array(6).fill("light"));
  });
});
