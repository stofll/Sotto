// Pure parts of the processing drawings and of the draft's decoding glyphs:
// what to draw for a moment in time, apart from any canvas or DOM, so the
// shapes can be tested.

import { orderedDither, putTone, type ToneTable } from "./pixelDither";

export const GLYPHS = "#%&@*+=<>/\\|{}[]?!$~";
export const randomGlyph = (random: () => number = Math.random) => GLYPHS[Math.floor(random() * GLYPHS.length)];

/** `chars` with every character but a space replaced by a glyph. */
export function scramble(chars: string, glyph: () => string) {
  let out = "";
  for (const char of chars) out += char === " " ? " " : glyph();
  return out;
}

/** The streaming hypothesis split into what is settled and its last characters, which may still change. */
export function decodeTail(text: string, count: number) {
  const body = text.trimEnd(), size = Math.min(count, body.length);
  return { head: body.slice(0, body.length - size), tail: body.slice(body.length - size) };
}

/** The glyph front's width, in characters. */
export const FRONT_WIDTH = 5;
/** Seconds one sweep of the front takes at normal pace. */
export const DECODE_PERIOD = 1.8;

/** The last `chars` characters of `text` on one line: all a row has room for. */
export function decodeWindow(text: string, chars: number) {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > chars ? flat.slice(flat.length - chars) : flat;
}

/** Where the front is, in characters, `seconds` after processing began. It sweeps left to right and starts over. */
export const decodeFront = (seconds: number, length: number) => ((seconds / DECODE_PERIOD) % 1) * (length + FRONT_WIDTH);

/** `text` split around the front: resolved before it, glyphs on it, the dim hypothesis still ahead. */
export function decodeFrame(text: string, front: number, glyph: () => string) {
  const at = Math.max(0, Math.min(text.length, Math.floor(front))), end = Math.min(text.length, at + FRONT_WIDTH);
  return { done: text.slice(0, at), glyphs: scramble(text.slice(at, end), glyph), rest: text.slice(end) };
}

/** Rings that spread from the middle of the buffer, dithered to four tones. */
export function paintWaves(data: Uint8ClampedArray, width: number, height: number, seconds: number, tones: ToneTable) {
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const distance = Math.hypot(x - width / 2, (y - height / 2) * 1.2) / height;
      const intensity = (0.5 + 0.5 * Math.cos(distance * 4.4 - seconds * 7)) * Math.exp(-distance * 0.16) * 0.95;
      const step = orderedDither(Math.max(0.06, intensity), x, y, 4), at = (y * width + x) * 4;
      if (step === 0) data[at + 3] = 0; else putTone(data, at, tones, 0.2 + 0.8 * step / 4);
    }
  }
}

/** The scan line's left edge: it sweeps across and back. */
export const scanX = (seconds: number, width: number) => (0.5 + 0.5 * Math.sin(seconds * 2.4)) * Math.max(0, width - 2);

export type BarCell = { char: string; tone: number; alpha: number };

/** `[  ==#=  ]`: a short bar bouncing between two brackets over a row of dots. */
export function barCells(columns: number, seconds: number): BarCell[] {
  const inner = columns - 2, length = Math.max(2, Math.min(5, Math.round(inner * 0.4)));
  const travel = (seconds * 0.9) % 2, head = (travel < 1 ? travel : 2 - travel) * Math.max(0, inner - length);
  return Array.from({ length: columns }, (_, column) => {
    if (column === 0) return { char: "[", tone: 0.5, alpha: 1 };
    if (column === columns - 1) return { char: "]", tone: 0.5, alpha: 1 };
    const k = column - 1 - head;
    if (k >= 0 && k < length) return { char: k > 0.5 && k < length - 1.5 ? "#" : "=", tone: 0.95, alpha: 1 };
    return { char: "·", tone: 0.5, alpha: 0.25 };
  });
}

export type WaveDot = { x: number; y: number; radius: number; tone: 0 | 1 | 2 };

/** A diagonal dot lattice whose dots swell as a wave runs across it. */
export function wavefrontDots(width: number, height: number, seconds: number, cell = 5): WaveDot[] {
  const diagonal = Math.SQRT1_2, reach = Math.ceil(Math.hypot(width, height) / cell / 2) + 1, dots: WaveDot[] = [];
  for (let i = -reach; i <= reach; i++) {
    for (let j = -reach; j <= reach; j++) {
      const x = width / 2 + (i - j) * diagonal * cell, y = height / 2 + (i + j) * diagonal * cell;
      if (x < -cell || x > width + cell || y < -cell || y > height + cell) continue;
      const v = y / height * 2 - 1;
      const intensity = Math.max(0.06, (0.5 + 0.5 * Math.sin(x / width * 9 - seconds * 6)) * Math.exp(-((v * 1.2) ** 2)));
      const radius = cell * 0.66 * Math.sqrt(intensity);
      if (radius < 0.35) continue;
      dots.push({ x, y, radius, tone: intensity > 0.6 ? 2 : intensity > 0.3 ? 1 : 0 });
    }
  }
  return dots;
}
