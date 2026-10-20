// Painters for the pixel-art level drawings. Each one draws the recording
// state from the level history (newest reading last) and is pure apart from
// the buffer it fills, so the shapes can be tested without a canvas.

import { clamp01, errorDiffusion, historyAt, orderedDither, putTone, smoothstep, toneIndex, type ToneTable } from "./pixelDither";

export const PIXEL_LEVELS = ["dorb", "tide", "static", "glitch", "lava"] as const;
export type PixelLevel = typeof PIXEL_LEVELS[number];

/** The buffer size a drawing paints for a canvas of `width` x `height` CSS px: one buffer pixel covers several screen pixels. */
export function bufferSize(draw: PixelLevel, width: number, height: number): [number, number] {
  switch (draw) {
    case "dorb": return [32, 32];
    case "tide": case "lava": return [Math.max(2, Math.round(width / 2)), Math.max(2, Math.round(height / 2))];
    case "glitch": return [Math.max(2, Math.round(width / 1.5)), Math.max(2, Math.round(height / 1.5))];
    case "static": return [Math.max(2, Math.round(width)), Math.max(2, Math.round(height))];
  }
}

const clear = (data: Uint8ClampedArray, at: number) => { data[at + 3] = 0; };

/** A ball of light, dithered to four tones; the voice swells it and shakes its edge. */
export function paintOrb(data: Uint8ClampedArray, side: number, level: number, seconds: number, tones: ToneTable) {
  const radius = 0.4 + 0.36 * level;
  for (let y = 0; y < side; y++) {
    for (let x = 0; x < side; x++) {
      const u = (x + 0.5) / side * 2 - 1, v = (y + 0.5) / side * 2 - 1, distance = Math.hypot(u, v), angle = Math.atan2(v, u);
      const edge = radius * (1 + level * (0.09 * Math.sin(angle * 3 + seconds * 4) + 0.05 * Math.sin(angle * 5 - seconds * 3)));
      let intensity: number;
      if (distance < edge) {
        const z = Math.sqrt(1 - (distance / edge) ** 2), nx = u / edge, ny = v / edge;
        intensity = 0.18 + 0.82 * Math.max(0, -0.5 * nx - 0.55 * ny + 0.67 * z);
      } else intensity = (0.25 + 0.6 * level) * Math.exp(-(distance - edge) * 7);
      const step = orderedDither(intensity, x, y, 4), at = (y * side + x) * 4;
      if (step === 0) clear(data, at); else putTone(data, at, tones, step / 4);
    }
  }
}

/** A swell that widens with the voice, the newest reading at the right edge. */
export function paintTide(data: Uint8ClampedArray, width: number, height: number, levels: ArrayLike<number>, tones: ToneTable) {
  for (let x = 0; x < width; x++) {
    const amplitude = Math.sqrt(historyAt(levels, (width - 1 - x) * 0.9));
    for (let y = 0; y < height; y++) {
      const v = (y + 0.5) / height * 2 - 1;
      const intensity = Math.exp(-((v / (0.12 + amplitude * 0.95)) ** 2) * 1.6) * (0.25 + 0.75 * amplitude) * (0.45 + 0.55 * x / width);
      const step = orderedDither(Math.max(0.07, intensity * 1.15), x, y, 4), at = (y * width + x) * 4;
      if (step === 0) clear(data, at); else putTone(data, at, tones, 0.2 + 0.8 * step / 4);
    }
  }
}

/** The densest share of pixels a static drawing lights, so a full level stays a texture and not a flashing field. */
export const STATIC_MAX_DENSITY = 0.6;

/** Grain whose density and spread follow the voice. `random` is injectable for tests. */
export function paintStatic(data: Uint8ClampedArray, width: number, height: number, levels: ArrayLike<number>, tones: ToneTable, random: () => number = Math.random) {
  for (let x = 0; x < width; x++) {
    const amplitude = Math.sqrt(historyAt(levels, (width - 1 - x) * 0.45));
    for (let y = 0; y < height; y++) {
      const v = (y + 0.5) / height * 2 - 1, at = (y * width + x) * 4;
      const energy = Math.exp(-((v / (0.08 + amplitude * 0.95)) ** 2) * 1.5) * (0.2 + 0.8 * amplitude) * (0.35 + 0.65 * x / width);
      if (random() < Math.min(STATIC_MAX_DENSITY, energy * 0.85 + 0.025)) putTone(data, at, tones, 0.35 + 0.65 * Math.min(1, energy * (0.7 + random() * 0.5)));
      else clear(data, at);
    }
  }
}

export type GlitchState = { shifts: Float32Array; next: number };
export const glitchState = (): GlitchState => ({ shifts: new Float32Array(16), next: 0 });

/**
 * Re-roll which bands of rows are torn sideways. The tears are rarer, shorter
 * and a little less far than a full-level concept would give, and none at all
 * when `calm`, so the loudest speech never turns into a strobe.
 */
export function tearGlitch(state: GlitchState, seconds: number, level: number, calm: boolean, random: () => number = Math.random) {
  if (seconds <= state.next) return;
  for (let band = 0; band < state.shifts.length; band++) {
    const torn = !calm && random() < level * 0.6;
    state.shifts[band] = torn ? (random() - 0.5) * 140 * (0.2 * level + 0.03) : 0;
  }
  state.next = seconds + 0.09 + random() * 0.12;
}

/** A trace that tears at the loud parts, its colour channels drifting apart. */
export function paintGlitch(data: Uint8ClampedArray, width: number, height: number, levels: ArrayLike<number>, level: number, state: GlitchState, tones: ToneTable) {
  const split = Math.round(1 + level * 4);
  const mask = (x: number, y: number) => {
    if (x < 0 || x >= width) return 0;
    const v = (y + 0.5) / height * 2 - 1, amplitude = Math.sqrt(historyAt(levels, (width - 1 - x) * 0.65));
    return Math.abs(v) < 0.08 + amplitude * 0.9 ? 0.35 + 0.65 * x / width : 0;
  };
  for (let y = 0; y < height; y++) {
    const shift = Math.round(state.shifts[Math.floor(y / 3) % state.shifts.length]);
    for (let x = 0; x < width; x++) {
      const red = mask(x - shift - split, y), green = mask(x - shift, y), blue = mask(x - shift + split, y), at = (y * width + x) * 4;
      if (!red && !green && !blue) { clear(data, at); continue; }
      data[at] = red ? tones[toneIndex(red)] : 0;
      data[at + 1] = green ? tones[toneIndex(green) + 1] : 0;
      data[at + 2] = blue ? tones[toneIndex(blue) + 2] : 0;
      data[at + 3] = 255;
    }
  }
}

export type LavaScratch = { intensity: Float32Array; on: Uint8Array; error: Float32Array };
export const lavaScratch = (width: number, height: number): LavaScratch => ({
  intensity: new Float32Array(width * height), on: new Uint8Array(width * height), error: new Float32Array((width + 2) * (height + 1)),
});

/** Five blobs that swell with the voice, merged and dithered to one bit. */
export function paintLava(data: Uint8ClampedArray, width: number, height: number, levels: ArrayLike<number>, seconds: number, scratch: LavaScratch, tones: ToneTable) {
  const balls: Array<[number, number, number]> = [];
  for (let k = 0; k < 5; k++) {
    const x = (k + 0.5) / 5 * width + Math.sin(seconds * (0.6 + k * 0.17) + k * 2) * width * 0.08;
    const y = height / 2 + Math.sin(seconds * (0.9 + k * 0.23) + k) * height * 0.18;
    const radius = 1.5 + 7 * (height / 16) * Math.sqrt(historyAt(levels, (width - x) * 0.9));
    balls.push([x, y, radius * radius]);
  }
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let field = 0;
      for (const [bx, by, r2] of balls) field += r2 / ((x - bx) ** 2 + ((y - by) * 1.6) ** 2 + 1);
      scratch.intensity[y * width + x] = Math.max(0.035, smoothstep(0.35, 2.4, field) * (0.45 + 0.55 * x / width));
    }
  }
  errorDiffusion(scratch.intensity, width, height, scratch.on, scratch.error);
  for (let index = 0; index < width * height; index++) {
    if (scratch.on[index]) putTone(data, index * 4, tones, 0.5 + 0.5 * Math.min(1, scratch.intensity[index] * 1.2));
    else clear(data, index * 4);
  }
}

export type HalftoneDot = { x: number; y: number; radius: number; tone: 0 | 1 | 2 };

export const HALFTONE_CELL = 5;

/** A 45 degree dot screen: the voice sets each dot's size, never its colour. */
export function halftoneDots(width: number, height: number, levels: ArrayLike<number>): HalftoneDot[] {
  const dots: HalftoneDot[] = [];
  const cell = HALFTONE_CELL, c = Math.SQRT1_2, reach = Math.ceil(Math.hypot(width, height) / cell / 2) + 1;
  for (let i = -reach; i <= reach; i++) {
    for (let j = -reach; j <= reach; j++) {
      const x = width / 2 + (i * c - j * c) * cell, y = height / 2 + (i * c + j * c) * cell;
      if (x < -cell || x > width + cell || y < -cell || y > height + cell) continue;
      const xn = clamp01(x / width), v = y / height * 2 - 1, amplitude = Math.sqrt(historyAt(levels, (1 - xn) * width * 0.35));
      const intensity = Math.max(0.05, Math.exp(-((v / (0.12 + amplitude * 0.95)) ** 2) * 1.4) * (0.2 + 0.8 * amplitude) * (0.4 + 0.6 * xn));
      const radius = cell * 0.66 * Math.sqrt(Math.min(1, intensity * 1.2));
      if (radius < 0.35) continue;
      dots.push({ x, y, radius, tone: intensity > 0.6 ? 2 : intensity > 0.3 ? 1 : 0 });
    }
  }
  return dots;
}

export const COLUMN_RAMP = ".:-=+*#%@";
export const COLUMN_ROWS = 3;

/** The character for one cell of the columns drawing, or null for the dim dot of an empty cell. `amplitude` is the column's 0..1 level. */
export function columnGlyph(amplitude: number, row: number): { char: string; tone: number } | null {
  const value = amplitude * 1.5 - Math.abs(row - (COLUMN_ROWS - 1) / 2) * 0.6;
  if (value <= 0.04) return null;
  return { char: COLUMN_RAMP[Math.min(COLUMN_RAMP.length - 1, Math.floor(value * COLUMN_RAMP.length))], tone: 0.45 + 0.55 * Math.min(1, value) };
}

/** How many radial ticks the ring shell's rim carries. */
export const RING_TICKS = 40;

/**
 * The amplitudes (0..1) of the ring rim's ticks, clockwise from the top.
 * Recording lays the level history round the rim with the newest reading
 * just before the top; processing runs a comet with a fading tail round it
 * (`seconds` is its clock); anything else leaves the rim at rest.
 */
export function ringTicks(mode: "recording" | "processing" | "idle", levels: ArrayLike<number>, seconds: number, count = RING_TICKS): number[] {
  return Array.from({ length: count }, (_, tick) => {
    if (mode === "recording") return Math.sqrt(clamp01(historyAt(levels, (count - 1 - tick) * 2)));
    if (mode === "processing") {
      // How far behind the comet's head this tick is, as a share of the rim.
      const behind = ((tick / count - (seconds * 0.8) % 1) + 2) % 1;
      return Math.max(0, 1 - (1 - behind) * 4);
    }
    return 0;
  });
}
