import { describe, expect, it } from "vitest";
import { toneTable } from "./pixelDither";
import { DECODE_PERIOD, FRONT_WIDTH, barCells, decodeFrame, decodeFront, decodeTail, decodeWindow, paintWaves, randomGlyph, scanX, scramble, wavefrontDots } from "./processPainters";

const glyph = () => "#";

describe("decoding glyphs", () => {
  it("scrambles every character but the spaces", () => {
    expect(scramble("a b c", glyph)).toBe("# # #");
    expect(scramble("", glyph)).toBe("");
  });

  it("only ever picks a glyph from the set", () => {
    for (let index = 0; index < 50; index++) expect(randomGlyph(() => index / 50)).toMatch(/^[^a-zA-Z0-9\s]$/);
    expect(randomGlyph(() => 0.9999999)).not.toBe("");
  });

  it("splits the last characters off the draft and leaves a short draft whole", () => {
    expect(decodeTail("давай перенесём релиз", 3)).toEqual({ head: "давай перенесём ре", tail: "лиз" });
    expect(decodeTail("да", 3)).toEqual({ head: "", tail: "да" });
    expect(decodeTail("ok  ", 3)).toEqual({ head: "", tail: "ok" });
    expect(decodeTail("", 3)).toEqual({ head: "", tail: "" });
  });
});

describe("decode front", () => {
  it("shows the last characters on one line and keeps a short text whole", () => {
    expect(decodeWindow("  a\n b   c ", 20)).toBe("a b c");
    expect(decodeWindow("one two three four", 9)).toBe("hree four");
    expect(decodeWindow("hello", 9)).toBe("hello");
  });

  it("sweeps from the start to past the end and starts over", () => {
    expect(decodeFront(0, 20)).toBe(0);
    expect(decodeFront(DECODE_PERIOD / 2, 20)).toBeCloseTo((20 + FRONT_WIDTH) / 2);
    expect(decodeFront(DECODE_PERIOD * 3, 20)).toBeCloseTo(0);
    for (let seconds = 0; seconds < 10; seconds += 0.37) {
      const front = decodeFront(seconds, 20);
      expect(front).toBeGreaterThanOrEqual(0);
      expect(front).toBeLessThan(20 + FRONT_WIDTH);
    }
  });

  it("resolves the text behind the front, scrambles it on the front and keeps it dim ahead", () => {
    const frame = decodeFrame("ab cdefghij", 3, glyph);
    expect(frame).toEqual({ done: "ab ", glyphs: "#####", rest: "hij" });
    // Whatever the front does, the three parts together are the text.
    for (const front of [-4, 0, 2.5, 9, 40]) {
      const part = decodeFrame("ab cdefghij", front, glyph);
      expect(part.done.length + part.glyphs.length + part.rest.length).toBe(11);
    }
    expect(decodeFrame("abc", 40, glyph)).toEqual({ done: "abc", glyphs: "", rest: "" });
    expect(decodeFrame("", 3, glyph)).toEqual({ done: "", glyphs: "", rest: "" });
  });
});

describe("processing painters", () => {
  const tones = toneTable(55, 0.14);

  it("paints rings that move: some pixels lit, some empty, and a different picture a moment later", () => {
    const width = 31, height = 13, a = new Uint8ClampedArray(width * height * 4).fill(255), b = new Uint8ClampedArray(a);
    paintWaves(a, width, height, 0, tones);
    paintWaves(b, width, height, 0.25, tones);
    const lit = (data: Uint8ClampedArray) => { let count = 0; for (let at = 3; at < data.length; at += 4) if (data[at]) count++; return count; };
    expect(lit(a)).toBeGreaterThan(0);
    expect(lit(a)).toBeLessThan(width * height);
    expect(Array.from(a)).not.toEqual(Array.from(b));
  });

  it("keeps the scan line inside the canvas whatever the time", () => {
    for (let seconds = 0; seconds < 20; seconds += 0.13) {
      expect(scanX(seconds, 60)).toBeGreaterThanOrEqual(0);
      expect(scanX(seconds, 60)).toBeLessThanOrEqual(58);
    }
    expect(scanX(1, 1)).toBeGreaterThanOrEqual(0);
  });

  it("draws the bar between brackets and bounces it without leaving them", () => {
    for (let seconds = 0; seconds < 8; seconds += 0.21) {
      const cells = barCells(12, seconds);
      expect(cells).toHaveLength(12);
      expect(cells[0].char).toBe("[");
      expect(cells[11].char).toBe("]");
      const bar = cells.slice(1, -1).filter((cell) => cell.char === "#" || cell.char === "=");
      expect(bar.length).toBeGreaterThanOrEqual(2);
    }
    expect(barCells(12, 0)[1].char).not.toBe("·");
    expect(barCells(12, 1 / 0.9)[10].char).not.toBe("·");
  });

  it("copes with a row too narrow for a bar", () => {
    expect(() => barCells(2, 1)).not.toThrow();
    expect(() => barCells(0, 1)).not.toThrow();
  });

  it("lays a dot lattice inside the canvas, with the brightest dots biggest", () => {
    const dots = wavefrontDots(62, 26, 0.4);
    expect(dots.length).toBeGreaterThan(10);
    for (const dot of dots) {
      expect(dot.x).toBeGreaterThanOrEqual(-5);
      expect(dot.x).toBeLessThanOrEqual(67);
      expect(dot.radius).toBeLessThanOrEqual(5 * 0.66 + 1e-9);
    }
    const mean = (tone: number) => { const group = dots.filter((dot) => dot.tone === tone); return group.reduce((sum, dot) => sum + dot.radius, 0) / (group.length || 1); };
    expect(mean(2)).toBeGreaterThan(mean(0));
  });
});
