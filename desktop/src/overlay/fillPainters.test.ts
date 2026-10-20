import { describe, expect, it } from "vitest";
import { fbm, fillGrain, hash, idleEnergy, paintSmoke, smokeBufferSize, smokeRate, valueNoise } from "./fillPainters";
import { toneTable } from "./pixelDither";

describe("noise", () => {
  it("is deterministic and stays in range", () => {
    expect(hash(3, 7)).toBe(hash(3, 7));
    expect(hash(3, 7)).not.toBe(hash(7, 3));
    for (let i = 0; i < 200; i++) {
      const x = i * 0.37 - 20, y = i * 0.91 + 3;
      for (const value of [hash(i, -i), valueNoise(x, y), fbm(x, y)]) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("is continuous: a small step changes the value a little", () => {
    expect(Math.abs(fbm(1.2, 2.3) - fbm(1.201, 2.3))).toBeLessThan(0.02);
  });
});

describe("paintSmoke", () => {
  const tones = toneTable(55, 0.14);
  const paint = (energy: number, time = 1) => {
    const data = new Uint8ClampedArray(24 * 12 * 4);
    paintSmoke(data, 24, 12, { time, drift: 0, energy }, tones);
    return data;
  };
  const alphaSum = (data: Uint8ClampedArray) => data.reduce((sum, value, at) => sum + (at % 4 === 3 ? value : 0), 0);

  it("covers more of the shell with the voice, never beyond the cap that keeps text readable", () => {
    const quiet = paint(0.05), loud = paint(1);
    expect(alphaSum(loud)).toBeGreaterThan(alphaSum(quiet));
    for (let at = 3; at < loud.length; at += 4) expect(loud[at]).toBeLessThanOrEqual(0.6 * 255 + 1);
  });

  it("paints two parts of one field as the whole field cut in two, so the split's capsules line up", () => {
    const frame = { time: 2, drift: 0.3, energy: 0.7 };
    const whole = new Uint8ClampedArray(24 * 12 * 4);
    paintSmoke(whole, 24, 12, frame, tones);
    const left = new Uint8ClampedArray(10 * 12 * 4), right = new Uint8ClampedArray(14 * 12 * 4);
    paintSmoke(left, 10, 12, frame, tones, { x: 0, width: 24 });
    paintSmoke(right, 14, 12, frame, tones, { x: 10, width: 24 });
    for (let y = 0; y < 12; y++) {
      expect(left.slice(y * 40, y * 40 + 40)).toEqual(whole.slice(y * 96, y * 96 + 40));
      expect(right.slice(y * 56, y * 56 + 56)).toEqual(whole.slice(y * 96 + 40, y * 96 + 96));
    }
  });

  it("moves with smoke time and is the same for the same frame", () => {
    expect(paint(0.5, 1)).toEqual(paint(0.5, 1));
    expect(paint(0.5, 1)).not.toEqual(paint(0.5, 4));
  });
});

describe("smoke pacing", () => {
  it("runs faster with the voice and crawls without one", () => {
    expect(smokeRate(true, 1)).toBeGreaterThan(smokeRate(true, 0));
    expect(smokeRate(false, 1)).toBe(smokeRate(false, 0));
    for (let drift = 0; drift < 10; drift += 0.3) expect(idleEnergy(drift)).toBeGreaterThan(0.05);
  });

  it("sizes the buffer to a quarter of the shell and never to nothing", () => {
    expect(smokeBufferSize(392, 112)).toEqual([98, 28]);
    expect(smokeBufferSize(0, 0)).toEqual([2, 2]);
  });
});

describe("fillGrain", () => {
  it("fills grey specks at the given alpha", () => {
    const data = new Uint8ClampedArray(16);
    let next = 0;
    fillGrain(data, () => (next += 0.25) % 1, 34);
    for (let at = 0; at < 16; at += 4) {
      expect(data[at]).toBe(data[at + 1]);
      expect(data[at]).toBe(data[at + 2]);
      expect(data[at + 3]).toBe(34);
    }
  });
});
