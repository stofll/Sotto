import { describe, expect, it } from "vitest";
import { binaryDigits, clockCells, clockSize, GLYPH_COLUMNS, GLYPH_ROWS, segmentLayout } from "./clockPainters";

describe("clockCells", () => {
  it("lays four columns to a character and lights the strokes of each digit", () => {
    const { cells, columns } = clockCells("07");
    expect(columns).toBe(7);
    expect(cells).toHaveLength(2 * GLYPH_COLUMNS * GLYPH_ROWS);
    // A zero is a ring of twelve lit cells and a seven has seven.
    expect(cells.filter((cell) => cell.column < 3 && cell.lit)).toHaveLength(12);
    expect(cells.filter((cell) => cell.column >= 4 && cell.lit)).toHaveLength(7);
  });

  it("draws a dash for the loading clock and nothing for a character it does not know", () => {
    expect(clockCells("--:--").cells.filter((cell) => cell.lit && cell.column === 0)).toHaveLength(1);
    expect(clockCells("x").cells.some((cell) => cell.lit)).toBe(false);
    expect(clockCells("")).toEqual({ cells: [], columns: 0 });
  });
});

describe("segmentLayout", () => {
  it("lights the segments each digit needs and keeps the colon as two dots", () => {
    const { glyphs } = segmentLayout("18:");
    expect(glyphs.map((glyph) => glyph.kind)).toEqual(["digit", "digit", "colon"]);
    expect(glyphs[0].kind === "digit" && glyphs[0].on).toBe("bc");
    expect(glyphs[1].kind === "digit" && glyphs[1].on).toBe("abcdefg");
    expect(glyphs[2].kind === "colon" && glyphs[2].dots).toHaveLength(2);
  });

  it("grows with the text, so an hours-long limit still fits its canvas", () => {
    expect(segmentLayout("1440:00").width).toBeGreaterThan(segmentLayout("00:00").width);
    expect(clockSize("seg", "1440:00")[0]).toBeGreaterThan(clockSize("seg", "00:00")[0]);
    expect(segmentLayout("").width).toBe(0);
  });
});

describe("binaryDigits", () => {
  it("writes each digit as four bits and skips the colon", () => {
    expect(binaryDigits("09:15")).toEqual([
      [false, false, false, false], [true, false, false, true], [false, false, false, true], [false, true, false, true],
    ]);
    expect(binaryDigits("--:--")).toEqual([]);
  });
});
