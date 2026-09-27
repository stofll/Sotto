import { describe, expect, it } from "vitest";
import { brightnessStep, MATRIX_PROCESS, matrixGrid, paintProcess, paintSpeech, speechState } from "./dotMatrix";

const lit = (cells: Float32Array) => cells.reduce((sum, value) => sum + (brightnessStep(value) > 0 ? 1 : 0), 0);

describe("matrixGrid", () => {
  it("keeps a square grid at the requested density", () => {
    const grid = matrixGrid(40, 40, 7, true);
    expect([grid.cols, grid.rows]).toEqual([7, 7]);
    expect(grid.pitch).toBeCloseTo(40 / 7);
  });

  it("fills a row with as many columns as the height allows", () => {
    const grid = matrixGrid(200, 28, 7, false);
    expect(grid.rows).toBe(7);
    expect(grid.cols).toBe(Math.floor(200 / 4));
    expect(grid.offsetX).toBeGreaterThanOrEqual(0);
  });

  it("walks every cell once, outer ring first", () => {
    for (const [cols, rows] of [[5, 5], [9, 9], [50, 7]]) {
      const grid = matrixGrid(cols * 4, rows * 4, rows, cols === rows);
      expect(new Set(grid.spiral).size).toBe(cols * rows);
      const ring = grid.spiral.slice(0, grid.perimeter);
      expect(ring.every((index) => {
        const x = index % cols, y = Math.floor(index / cols);
        return x === 0 || y === 0 || x === cols - 1 || y === rows - 1;
      })).toBe(true);
    }
  });
});

describe("brightnessStep", () => {
  it("draws only four steps and clamps out-of-range values", () => {
    expect([-1, 0, 0.2, 0.5, 0.8, 1, 3].map(brightnessStep)).toEqual([0, 0, 1, 2, 2, 3, 3]);
  });
});

describe("paintSpeech", () => {
  it("lights more of the rings as the voice gets louder", () => {
    const grid = matrixGrid(40, 40, 9, true);
    const counts = [0, 0.1, 0.4, 0.9].map((level) => {
      const cells = new Float32Array(81);
      paintSpeech(grid, cells, "rings", [level], 0, speechState());
      return lit(cells);
    });
    expect(counts[0]).toBe(0);
    for (let index = 1; index < counts.length; index++) expect(counts[index]).toBeGreaterThan(counts[index - 1]);
  });

  it("fades back to dark once the voice stops", () => {
    const grid = matrixGrid(40, 40, 7, true);
    const cells = new Float32Array(49);
    const state = speechState();
    paintSpeech(grid, cells, "ripple", [0.1, 0.9], 0, state);
    expect(lit(cells)).toBeGreaterThan(0);
    for (let tick = 1; tick <= 40; tick++) paintSpeech(grid, cells, "ripple", [0], tick * 33, state);
    expect(lit(cells)).toBe(0);
  });

  it("draws the newest reading in the right-hand column of the wave", () => {
    const grid = matrixGrid(80, 20, 5, false);
    const cells = new Float32Array(grid.cols * grid.rows);
    paintSpeech(grid, cells, "wave", [...Array<number>(grid.cols - 1).fill(0), 1], 0, speechState());
    const column = (x: number) => Array.from({ length: grid.rows }, (_, y) => cells[y * grid.cols + x]);
    expect(column(grid.cols - 1).every((value) => value > 0)).toBe(true);
    expect(column(0).every((value) => value === 0)).toBe(true);
  });
});

describe("paintProcess", () => {
  it("keeps every processing pattern moving and never fully dark", () => {
    const grid = matrixGrid(40, 40, 7, true);
    for (const pattern of MATRIX_PROCESS) {
      const frames = [0, 0.2, 0.45].map((seconds) => {
        const cells = new Float32Array(49);
        paintProcess(grid, cells, pattern, seconds);
        expect(lit(cells)).toBeGreaterThan(0);
        return Array.from(cells, brightnessStep).join("");
      });
      expect(new Set(frames).size).toBeGreaterThan(1);
    }
  });

  it("runs the perimeter snake along the edge only", () => {
    const grid = matrixGrid(40, 40, 7, true);
    const cells = new Float32Array(49);
    paintProcess(grid, cells, "perimeter", 0.6);
    const inner = grid.spiral.slice(grid.perimeter);
    expect(inner.every((index) => cells[index] === 0)).toBe(true);
  });
});
