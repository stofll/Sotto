// The geometry of the clock drawings (seven segments, LED dots, character
// grid, binary dots), kept apart from the canvas that paints them so the
// layout can be tested.

/** A 3 x 5 bitmap per character, row by row. */
const FONT: Record<string, string> = {
  0: "111101101101111", 1: "010110010010111", 2: "111001111100111", 3: "111001111001111", 4: "101101111001001",
  5: "111100111001111", 6: "111100111101111", 7: "111001001001001", 8: "111101111101111", 9: "111101111001111",
  ":": "000010000010000", "-": "000000111000000",
};
export const GLYPH_COLUMNS = 3;
export const GLYPH_ROWS = 5;
const BLANK = "0".repeat(GLYPH_COLUMNS * GLYPH_ROWS);

export type Cell = { column: number; row: number; lit: boolean };

/** The dot-matrix cells of `text`: four columns a character, the last of them an empty gap. */
export function clockCells(text: string): { cells: Cell[]; columns: number } {
  const cells: Cell[] = [];
  Array.from(text).forEach((char, index) => {
    const bits = FONT[char] ?? BLANK;
    for (let row = 0; row < GLYPH_ROWS; row++) {
      for (let column = 0; column < GLYPH_COLUMNS; column++) cells.push({ column: index * 4 + column, row, lit: bits[row * GLYPH_COLUMNS + column] === "1" });
    }
  });
  return { cells, columns: Math.max(0, Array.from(text).length * 4 - 1) };
}

export const SEGMENT_DIGITS: Record<string, string> = {
  0: "abcdef", 1: "bc", 2: "abdeg", 3: "abcdg", 4: "bcfg", 5: "acdfg", 6: "acdefg", 7: "abc", 8: "abcdefg", 9: "abcdfg", "-": "g",
};
const SEGMENT = { width: 13, height: 22, thickness: 2.6, gap: 5, colon: 6 } as const;

export type Rect = readonly [x: number, y: number, width: number, height: number];
export type SegmentGlyph = { kind: "digit"; segments: Record<string, Rect>; on: string } | { kind: "colon"; dots: readonly Rect[] };

/** The seven-segment layout of `text`, in a box of `width` x `height` units. Its natural size is `SEGMENT_SIZE`; the caller scales it. */
export function segmentLayout(text: string): { glyphs: SegmentGlyph[]; width: number; height: number } {
  const { width: dw, height: dh, thickness: th, gap, colon } = SEGMENT;
  const half = dh / 2;
  let x = 0;
  const glyphs: SegmentGlyph[] = [];
  for (const char of Array.from(text)) {
    if (char === ":") {
      glyphs.push({ kind: "colon", dots: [[x + 1.5, 6, th, th], [x + 1.5, dh - 8, th, th]] });
      x += colon + gap;
      continue;
    }
    const long = half - 1.5 * th;
    glyphs.push({
      kind: "digit", on: SEGMENT_DIGITS[char] ?? "",
      segments: {
        a: [x + th, 0, dw - 2 * th, th], b: [x + dw - th, th, th, long], c: [x + dw - th, half + th / 2, th, long],
        d: [x + th, dh - th, dw - 2 * th, th], e: [x, half + th / 2, th, long], f: [x, th, th, long], g: [x + th, half - th / 2, dw - 2 * th, th],
      },
    });
    x += dw + gap;
  }
  return { glyphs, width: Math.max(0, x - gap), height: dh };
}

/** The four bits of each digit of `text` (colons are not digits), most significant first. */
export function binaryDigits(text: string): boolean[][] {
  return Array.from(text).filter((char) => char >= "0" && char <= "9")
    .map((char) => [3, 2, 1, 0].map((bit) => ((Number(char) >> bit) & 1) === 1));
}

/** How wide a clock drawing is for `text`, in CSS px: what the canvas is sized to. */
export type ClockDraw = "seg" | "led" | "glyph" | "binary";
const CELL = { led: 4.6, glyphWidth: 5.6, glyphHeight: 5.8, binary: 6.6, binaryGap: 6, margin: 10 };
export function clockSize(draw: ClockDraw, text: string): [number, number] {
  const length = Array.from(text).length;
  switch (draw) {
    case "seg": return [Math.ceil(segmentLayout(text).width) + CELL.margin * 2, 26];
    case "led": return [Math.ceil(Math.max(0, length * 4 - 1) * CELL.led) + CELL.margin * 2, 26];
    case "glyph": return [Math.ceil(Math.max(0, length * 4 - 1) * CELL.glyphWidth) + CELL.margin * 2, 30];
    case "binary": return [Math.ceil(binaryDigits(text).length * CELL.binary + CELL.binaryGap) + CELL.margin * 2, 30];
  }
}
export const CLOCK_CELL = CELL;
