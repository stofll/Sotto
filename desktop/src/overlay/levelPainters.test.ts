import { describe, expect, it } from "vitest";
import {
  RING_TICKS, STATIC_MAX_DENSITY, bufferSize, columnGlyph, glitchState, halftoneDots, lavaScratch, paintGlitch, paintLava, paintOrb, paintStatic, paintTide, ringTicks, tearGlitch,
} from "./levelPainters";
import { toneTable } from "./pixelDither";

const tones = toneTable(262, 0.19);
const history = (level: number) => Array(96).fill(level);
const buffer = (width: number, height: number) => new Uint8ClampedArray(width * height * 4);
const litCount = (data: Uint8ClampedArray) => { let lit = 0; for (let i = 3; i < data.length; i += 4) if (data[i] > 0) lit++; return lit; };
// A repeatable stream of numbers in [0, 1).
const seeded = (seed = 7) => () => { seed = (seed * 1664525 + 1013904223) >>> 0; return seed / 4294967296; };

describe("bufferSize", () => {
  it("keeps the orb at 32 pixels and scales rows down by the drawing's own factor", () => {
    expect(bufferSize("dorb", 70, 70)).toEqual([32, 32]);
    expect(bufferSize("tide", 200, 32)).toEqual([100, 16]);
    expect(bufferSize("glitch", 150, 30)).toEqual([100, 20]);
    expect(bufferSize("static", 58, 26)).toEqual([58, 26]);
    expect(bufferSize("lava", 1, 1)).toEqual([2, 2]);
  });
});

describe("pixel drawings", () => {
  it("swell the orb with the voice", () => {
    const quiet = buffer(32, 32), loud = buffer(32, 32);
    paintOrb(quiet, 32, 0, 1, tones);
    paintOrb(loud, 32, 1, 1, tones);
    expect(litCount(loud)).toBeGreaterThan(litCount(quiet));
  });

  it("draws the tide wider where the history is loud, newest at the right", () => {
    const data = buffer(60, 16), levels = [...history(0), ...history(1)].slice(-96);
    paintTide(data, 60, 16, levels, tones);
    const column = (x: number) => { let lit = 0; for (let y = 0; y < 16; y++) if (data[(y * 60 + x) * 4 + 3] > 0) lit++; return lit; };
    expect(column(59)).toBeGreaterThan(column(0));
    const silent = buffer(60, 16);
    paintTide(silent, 60, 16, history(0), tones);
    expect(litCount(silent)).toBeLessThan(litCount(data));
  });

  it("keeps static a texture at full level and almost empty in silence", () => {
    const loud = buffer(120, 30), quiet = buffer(120, 30);
    paintStatic(loud, 120, 30, history(1), tones, seeded());
    paintStatic(quiet, 120, 30, history(0), tones, seeded());
    expect(litCount(loud) / (120 * 30)).toBeLessThanOrEqual(STATIC_MAX_DENSITY + 0.03);
    expect(litCount(loud)).toBeGreaterThan(litCount(quiet) * 4);
  });

  it("lights more lava the louder the voice", () => {
    const run = (level: number) => {
      const data = buffer(80, 16);
      paintLava(data, 80, 16, history(level), 2, lavaScratch(80, 16), tones);
      return litCount(data);
    };
    expect(run(1)).toBeGreaterThan(run(0));
  });
});

describe("glitch", () => {
  it("never tears when calm, and bounds the tear when loud", () => {
    const state = glitchState();
    tearGlitch(state, 1, 1, true, () => 0);
    expect(Array.from(state.shifts).every((shift) => shift === 0)).toBe(true);
    tearGlitch(state, 5, 1, false, seeded());
    const widest = Math.max(...Array.from(state.shifts, Math.abs));
    expect(widest).toBeGreaterThan(0);
    // The concept's full-level tear reaches about 21 buffer pixels either way.
    expect(widest).toBeLessThanOrEqual(0.5 * 140 * (0.2 + 0.03));
  });

  it("waits before it tears again", () => {
    const state = glitchState();
    tearGlitch(state, 1, 1, false, seeded());
    const first = Array.from(state.shifts), next = state.next;
    expect(next).toBeGreaterThan(1.08);
    tearGlitch(state, 1.01, 1, false, () => 0.99);
    expect(Array.from(state.shifts)).toEqual(first);
  });

  it("splits the colour channels apart at the edges of the trace", () => {
    const data = buffer(40, 12), state = glitchState();
    paintGlitch(data, 40, 12, history(1), 1, state, tones);
    expect(litCount(data)).toBeGreaterThan(0);
    // Somewhere a pixel is lit by one channel but not another.
    let split = false;
    for (let i = 0; i < data.length; i += 4) if (data[i + 3] && (data[i] === 0 || data[i + 1] === 0 || data[i + 2] === 0)) split = true;
    expect(split).toBe(true);
  });
});

describe("halftone", () => {
  it("makes the dots bigger where the history is louder", () => {
    const mean = (levels: number[]) => { const dots = halftoneDots(200, 32, levels); return dots.reduce((sum, dot) => sum + dot.radius, 0) / dots.length; };
    expect(mean(history(1))).toBeGreaterThan(mean(history(0)));
  });

  it("keeps every dot inside the canvas margin and gives it a tone", () => {
    for (const dot of halftoneDots(200, 32, history(0.5))) {
      expect(dot.x).toBeGreaterThanOrEqual(-5);
      expect(dot.x).toBeLessThanOrEqual(205);
      expect([0, 1, 2]).toContain(dot.tone);
    }
  });
});

describe("columnGlyph", () => {
  it("shows a dim dot for silence and denser characters for louder columns", () => {
    expect(columnGlyph(0, 1)).toBeNull();
    const ramp = ".:-=+*#%@";
    const quiet = columnGlyph(0.3, 1)!, loud = columnGlyph(1, 1)!;
    expect(ramp.indexOf(loud.char)).toBeGreaterThan(ramp.indexOf(quiet.char));
    expect(loud.char).toBe("@");
  });

  it("rises from the middle row outwards", () => {
    expect(columnGlyph(0.5, 1)).not.toBeNull();
    expect(columnGlyph(0.2, 0)).toBeNull();
    expect(columnGlyph(1, 0)!.tone).toBeLessThan(columnGlyph(1, 1)!.tone);
  });
});

describe("ringTicks", () => {
  it("lays the history round the rim with the newest reading at the end", () => {
    const levels = Array(96).fill(0);
    levels[95] = 1;
    const ticks = ringTicks("recording", levels, 0);
    expect(ticks).toHaveLength(RING_TICKS);
    expect(ticks[RING_TICKS - 1]).toBe(1);
    expect(ticks.slice(0, RING_TICKS - 1).every((tick) => tick === 0)).toBe(true);
  });

  it("keeps readings outside 0..1 on the rim", () => {
    expect(ringTicks("recording", Array(96).fill(4), 0).every((tick) => tick === 1)).toBe(true);
    expect(ringTicks("recording", Array(96).fill(-1), 0).every((tick) => tick === 0)).toBe(true);
  });

  it("runs a comet with a tail round the rim while processing, and moves it with time", () => {
    const at = (seconds: number) => ringTicks("processing", [], seconds);
    const lit = (ticks: number[]) => ticks.filter((tick) => tick > 0).length;
    expect(lit(at(0))).toBeGreaterThan(1);
    expect(lit(at(0))).toBeLessThan(RING_TICKS / 2);
    expect(Math.max(...at(0))).toBeGreaterThan(0.85);
    expect(at(0.5)).not.toEqual(at(0));
    // A full lap of the rim returns to the start.
    expect(at(1.25)).toEqual(at(0));
  });

  it("leaves the rim at rest otherwise", () => {
    expect(ringTicks("idle", Array(96).fill(1), 3).every((tick) => tick === 0)).toBe(true);
  });
});
