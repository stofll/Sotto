// The maths of the smoke fill and the grain texture, kept apart from the canvas
// components so it can be tested and timed without a DOM.

import { clamp01, putTone, smoothstep, type ToneTable } from "./pixelDither";

/** One buffer pixel covers this many CSS px: smoke is soft, so a coarse buffer scaled up smoothly is enough. */
export const SMOKE_CELL = 4;

/** A hash of two integers in [0, 1). */
export function hash(x: number, y: number) {
  let h = (Math.imul(x, 374761393) + Math.imul(y, 668265263)) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

/** Smoothly interpolated lattice noise in [0, 1]. */
export function valueNoise(x: number, y: number) {
  const xi = Math.floor(x), yi = Math.floor(y), xf = x - xi, yf = y - yi;
  const u = xf * xf * (3 - 2 * xf), v = yf * yf * (3 - 2 * yf);
  const a = hash(xi, yi), b = hash(xi + 1, yi), c = hash(xi, yi + 1), d = hash(xi + 1, yi + 1);
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
}

/** Four octaves of value noise, normalised to [0, 1]. */
export function fbm(x: number, y: number) {
  let sum = 0, amplitude = 0.5;
  for (let octave = 0; octave < 4; octave++) {
    sum += amplitude * valueNoise(x, y);
    x = x * 2.03 + 17.1; y = y * 2.03 + 9.2; amplitude *= 0.5;
  }
  return sum / 0.9375;
}

/** The smoke's size in buffer pixels for a shell of `width` x `height` CSS px. */
export const smokeBufferSize = (width: number, height: number): [number, number] =>
  [Math.max(2, Math.round(width / SMOKE_CELL)), Math.max(2, Math.round(height / SMOKE_CELL))];

/** Share of the shell the smoke may cover at full voice. It is capped so the words on top stay readable. */
const SMOKE_ALPHA = 0.6;

export type SmokeFrame = {
  /** Smoke time: runs faster with the voice. */
  time: number;
  /** A slow sideways drift that carries the smoke while there is no voice. */
  drift: number;
  /** The voice's smoothed energy, 0..1. */
  energy: number;
};

/** Where a buffer sits in a wider smoke field, in buffer pixels: the split's two capsules show two parts of one field. */
export type SmokeField = { x: number; width: number };

/** Domain-warped fbm smoke into an RGBA buffer, glowing from the bottom centre of `field` and brighter with `energy`. */
export function paintSmoke(data: Uint8ClampedArray, width: number, height: number, frame: SmokeFrame, tones: ToneTable, field: SmokeField = { x: 0, width }) {
  const { time, drift, energy } = frame;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const fx = x + field.x, px = fx / 26 + drift, py = y / 26;
      const q1 = fbm(px * 1.3 + time * 0.06, py * 1.3 - time * 0.04), q2 = fbm(px * 1.3 + 5.2 - time * 0.05, py * 1.3 + 1.3);
      const v = fbm(px + 2.6 * q1 * (0.7 + energy) + time * 0.1, py + 2.6 * q2 - time * 0.05);
      const glow = Math.exp(-(((fx / field.width - 0.5) * 2.2) ** 2) - (((1 - y / height) * 2.4) ** 2)) * energy;
      const intensity = smoothstep(0.32, 0.9, v) * (0.28 + 0.8 * energy) + glow * 0.55;
      putTone(data, (y * width + x) * 4, tones, Math.min(1, intensity * 0.95), Math.round(clamp01(intensity * 1.4) * SMOKE_ALPHA * 255));
    }
  }
}

/** The energy the smoke shows while the person is not speaking: a faint swell, so processing is not a still picture. */
export const idleEnergy = (drift: number) => 0.18 + 0.1 * Math.sin(drift * 8.8);

/** How fast smoke time runs: faster with the voice while recording, a steady crawl otherwise. */
export const smokeRate = (recording: boolean, energy: number) => recording ? 0.6 + 2.6 * energy : 0.5;

/** Monochrome noise, one grey in each byte triple with a fixed low alpha, for a grain tile. */
export function fillGrain(data: Uint8ClampedArray, random: () => number = Math.random, alpha = 34) {
  for (let at = 0; at < data.length; at += 4) {
    data[at] = data[at + 1] = data[at + 2] = random() * 255;
    data[at + 3] = alpha;
  }
}
