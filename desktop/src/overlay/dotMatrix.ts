// Dot-matrix drawing of the recording level. Every cell holds a brightness
// from 0 to 1 and is shown in one of four steps: the colour may fade, the dot
// itself never grows or blurs.

export const MATRIX_SPEECH = ["rings", "wave", "ripple", "field"] as const;
export const MATRIX_PROCESS = ["perimeter", "scan", "spiral", "sonar"] as const;
export const MATRIX_DENSITIES = [5, 7, 9] as const;
export type MatrixSpeech = typeof MATRIX_SPEECH[number];
export type MatrixProcess = typeof MATRIX_PROCESS[number];
export type MatrixDensity = typeof MATRIX_DENSITIES[number];

export type MatrixGrid = {
  cols: number;
  rows: number;
  pitch: number;
  offsetX: number;
  offsetY: number;
  /** Distance of each cell from the centre, 0 at the centre and about 1 at the far edge. */
  distance: Float32Array;
  /** Every cell, walking the outer ring clockwise first and then inward. */
  spiral: number[];
  /** How many leading entries of `spiral` form the outer ring. */
  perimeter: number;
};

export function matrixGrid(width: number, height: number, density: number, square: boolean): MatrixGrid {
  const rows = Math.max(1, density);
  const pitch = square ? Math.min(width, height) / rows : height / rows;
  const cols = square ? rows : Math.max(rows, Math.floor(width / pitch));
  const count = cols * rows;
  const centreX = (cols - 1) / 2, centreY = (rows - 1) / 2;
  const distance = new Float32Array(count);
  for (let index = 0; index < count; index++) {
    const dx = (index % cols - centreX) / Math.max(1, centreX);
    const dy = (Math.floor(index / cols) - centreY) / Math.max(1, centreY);
    // A wide row spreads sideways, so height counts for less there.
    distance[index] = square ? Math.hypot(dx, dy) / Math.SQRT2 : Math.min(1, Math.hypot(dx, dy * rows / cols));
  }
  return {
    cols, rows, pitch,
    offsetX: (width - cols * pitch) / 2,
    offsetY: (height - rows * pitch) / 2,
    distance, spiral: spiralOrder(cols, rows),
    perimeter: rows > 1 && cols > 1 ? 2 * (cols + rows) - 4 : cols * rows,
  };
}

function spiralOrder(cols: number, rows: number) {
  const order: number[] = [];
  let top = 0, bottom = rows - 1, left = 0, right = cols - 1;
  while (top <= bottom && left <= right) {
    for (let x = left; x <= right; x++) order.push(top * cols + x);
    for (let y = top + 1; y <= bottom; y++) order.push(y * cols + right);
    if (top < bottom) for (let x = right - 1; x >= left; x--) order.push(bottom * cols + x);
    if (left < right) for (let y = bottom - 1; y > top; y--) order.push(y * cols + left);
    top++; bottom--; left++; right--;
  }
  return order;
}

/** One of the four steps a cell is drawn with: 0 is the dim dot, 3 is full. */
export function brightnessStep(value: number) {
  return Math.round(Math.max(0, Math.min(1, value)) * 3);
}

export type SpeechState = { rings: Array<{ start: number; strength: number }>; average: number; lastRing: number };
export function speechState(): SpeechState {
  return { rings: [], average: 0, lastRing: -Infinity };
}

const RIPPLE_LIFE_MS = 900;

/**
 * Advance a speech pattern by one level reading. `levels` holds the recent
 * readings with the newest last; `now` is in milliseconds. Patterns that keep
 * a trail fade the previous frame instead of clearing it.
 */
export function paintSpeech(grid: MatrixGrid, cells: Float32Array, pattern: MatrixSpeech, levels: readonly number[], now: number, state: SpeechState) {
  const { cols, rows, distance } = grid;
  const count = cols * rows;
  const level = Math.sqrt(levels[levels.length - 1] ?? 0);
  const middle = (rows - 1) / 2;
  switch (pattern) {
    case "rings": {
      const reach = level * 1.15;
      for (let index = 0; index < count; index++) {
        const d = distance[index];
        cells[index] = Math.max(level > 0.06 && d <= reach ? 1 - 0.6 * d / (reach + 0.001) : 0, cells[index] * 0.55);
      }
      break;
    }
    case "wave": {
      // A square grid has few columns, so it skips every other reading to show a longer stretch.
      const stride = cols === rows ? 2 : 1;
      for (let index = 0; index < count; index++) {
        const x = index % cols, y = Math.floor(index / cols);
        const value = Math.sqrt(levels[levels.length - 1 - (cols - 1 - x) * stride] ?? 0);
        cells[index] = Math.abs(y - middle) < value * (rows / 2 + 0.4) ? 0.4 + 0.6 * x / Math.max(1, cols - 1) : 0;
      }
      break;
    }
    case "ripple": {
      state.average = state.average * 0.9 + level * 0.1;
      if (level > 0.2 && level > state.average + 0.08 && now - state.lastRing > 170) {
        state.rings.push({ start: now, strength: Math.min(1, level * 1.2) });
        state.lastRing = now;
      }
      state.rings = state.rings.filter((ring) => now - ring.start < RIPPLE_LIFE_MS);
      for (let index = 0; index < count; index++) {
        const d = distance[index];
        let value = d < level * 0.3 ? level : 0;
        for (const ring of state.rings) {
          const radius = (now - ring.start) / RIPPLE_LIFE_MS * 1.25;
          const band = 1 - Math.abs(d - radius) * rows * 0.9;
          if (band > 0) value = Math.max(value, band * ring.strength * (1 - radius / 1.25));
        }
        cells[index] = Math.max(value, cells[index] * 0.72);
      }
      break;
    }
    case "field": {
      const seconds = now / 1000, threshold = 1 - (0.1 + level * 0.95);
      for (let index = 0; index < count; index++) {
        const x = index % cols, y = Math.floor(index / cols);
        const noise = 0.5 + 0.5 * Math.sin(x * 0.8 + seconds * 1.6 + Math.sin(y * 0.9 - seconds * 0.9) * 1.8)
          * Math.cos(y * 0.7 - seconds * 1.2 + Math.sin(x * 0.45 + seconds * 0.7) * 1.4);
        cells[index] = Math.max(noise > threshold ? 0.35 + 0.65 * (noise - threshold) / (1 - threshold + 0.001) : 0, cells[index] * 0.6);
      }
      break;
    }
  }
}

/** Draw a processing pattern for the given time since processing began, in seconds. */
export function paintProcess(grid: MatrixGrid, cells: Float32Array, pattern: MatrixProcess, seconds: number) {
  const { cols, rows, distance, spiral, perimeter } = grid;
  const count = cols * rows;
  cells.fill(0);
  switch (pattern) {
    case "perimeter": {
      const length = Math.max(3, Math.round(perimeter / 3));
      const head = Math.floor(seconds * perimeter / 1.3);
      for (let step = 0; step < length; step++) {
        cells[spiral[((head - step) % perimeter + perimeter) % perimeter]] = 1 - step / length * 0.8;
      }
      break;
    }
    case "scan": {
      const phase = (seconds / 0.8) % 2;
      const column = (phase < 1 ? phase : 2 - phase) * (cols - 1);
      const width = cols === rows ? 1.6 : 3;
      for (let index = 0; index < count; index++) cells[index] = Math.max(0, 1 - Math.abs(index % cols - column) / width);
      break;
    }
    case "spiral": {
      const step = Math.floor(seconds * count / 1.1) % (2 * count);
      for (let order = 0; order < count; order++) {
        const lit = step < count ? order <= step : order > step - count;
        cells[spiral[order]] = lit ? (order === step % count ? 1 : 0.45) : 0;
      }
      break;
    }
    case "sonar": {
      for (let index = 0; index < count; index++) {
        let value = distance[index] < 0.12 ? 0.5 : 0;
        for (let wave = 0; wave < 3; wave++) {
          const radius = ((seconds + wave * 0.45) % 1.35) / 1.35 * 1.25;
          const band = 1 - Math.abs(distance[index] - radius) * rows * 0.85;
          if (band > 0) value = Math.max(value, band * (1 - radius / 1.25));
        }
        cells[index] = value;
      }
      break;
    }
  }
}
