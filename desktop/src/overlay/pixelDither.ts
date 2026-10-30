// Shared pixel helpers for the canvas drawings that paint a small buffer and
// scale it up: ordered and error-diffusion dithering, a tone table built from
// the overlay palette, and the buffer a drawing blits to its canvas.

const BAYER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5].map((value) => (value + 0.5) / 16);

/** The Bayer 4 x 4 threshold (0..1) at a pixel. */
export const bayerThreshold = (x: number, y: number) => BAYER[(y & 3) * 4 + (x & 3)];

export const clamp01 = (value: number) => Math.max(0, Math.min(1, value));

export function smoothstep(edge0: number, edge1: number, value: number) {
  const x = clamp01((value - edge0) / (edge1 - edge0));
  return x * x * (3 - 2 * x);
}

/** Ordered (Bayer 4 x 4) dither of an intensity 0..1 into `steps` tones. 0 means the pixel stays empty. */
export function orderedDither(intensity: number, x: number, y: number, steps: number) {
  const value = clamp01(intensity) * steps, base = Math.floor(value);
  return Math.min(steps, base + (value - base > bayerThreshold(x, y) ? 1 : 0));
}

/**
 * One-bit Floyd-Steinberg dither of `intensity` (row by row, `width` x `height`)
 * into `out`: 1 where a pixel is on. `scratch` holds the carried error and
 * must have `(width + 2) * (height + 1)` entries; it is cleared here.
 */
export function errorDiffusion(intensity: ArrayLike<number>, width: number, height: number, out: Uint8Array, scratch: Float32Array) {
  scratch.fill(0);
  const stride = width + 2;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const at = y * stride + x + 1, value = intensity[y * width + x] + scratch[at];
      const on = value > 0.5 ? 1 : 0, error = value - on;
      scratch[at + 1] += error * 7 / 16;
      scratch[at + stride - 1] += error * 3 / 16;
      scratch[at + stride] += error * 5 / 16;
      scratch[at + stride + 1] += error / 16;
      out[y * width + x] = on;
    }
  }
}

/** The reading `age` steps before the newest (the history keeps the newest last); older than the history repeats its oldest entry. */
export function historyAt(levels: ArrayLike<number>, age: number) {
  return levels[Math.max(0, levels.length - 1 - Math.round(age))] ?? 0;
}

/** OKLCH to sRGB bytes, so a pixel buffer gets the colours the overlay's CSS would. */
export function oklchToRgb(lightness: number, chroma: number, hue: number): [number, number, number] {
  const a = chroma * Math.cos(hue * Math.PI / 180), b = chroma * Math.sin(hue * Math.PI / 180);
  const l = (lightness + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (lightness - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (lightness - 0.0894841775 * a - 1.2914855480 * b) ** 3;
  const byte = (value: number) => {
    const x = clamp01(value);
    return Math.round(255 * (x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055));
  };
  return [
    byte(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
    byte(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
    byte(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
  ];
}

/** 256 tones, each three bytes, from the palette's dimmest (index 0) to its brightest. */
export type ToneTable = Uint8ClampedArray;

/**
 * The tone table for a palette hue and chroma. Light ink glows on a dark
 * shell; dark ink is for a light one, where a brighter tone is a darker mark.
 */
export function toneTable(hue: number, chroma: number, ink: "light" | "dark" = "light"): ToneTable {
  const table = new Uint8ClampedArray(768);
  for (let index = 0; index < 256; index++) {
    const x = index / 255;
    const lightness = ink === "light" ? 0.16 + 0.8 * x : 0.72 - 0.38 * x;
    // The top tones lose some colour so the brightest pixels read as highlights.
    const saturation = chroma * 1.3 * (ink === "light" && x > 0.78 ? 1 - (x - 0.78) / 0.22 * 0.8 : 1);
    table.set(oklchToRgb(lightness, saturation, hue), index * 3);
  }
  return table;
}

/** The ink for a shell fill: the light fills (light and paper) get dark marks, every other fill glows. */
export const inkForFill = (fill: string | null | undefined): "light" | "dark" => fill === "light" || fill === "paper" ? "dark" : "light";

/** What a canvas drawing needs from the overlay palette: its tones and the monospace font at the scene's scale. */
export type PaletteLook = { tones: ToneTable; font: string };

/**
 * A reader of the overlay palette from the canvas's CSS variables. The palette
 * can change while the overlay is open, so the result is re-read about once a
 * second instead of per frame, and the tone table is only rebuilt when the colour changed.
 */
export function paletteLook(canvas: HTMLCanvasElement): (now: number) => PaletteLook {
  let look: PaletteLook | null = null, readAt = -Infinity, key = "";
  return (now) => {
    if (look && now - readAt < 1000) return look;
    readAt = now;
    const style = getComputedStyle(canvas);
    const hue = parseFloat(style.getPropertyValue("--overlay-hue")), chroma = parseFloat(style.getPropertyValue("--overlay-chroma"));
    const ink = inkForFill(canvas.closest("[data-fill]")?.getAttribute("data-fill"));
    const fontSize = 11 * (parseFloat(style.getPropertyValue("--fs")) || 1);
    const font = `400 ${fontSize}px ${style.getPropertyValue("--font-mono").trim() || "monospace"}`;
    const nextKey = `${hue}:${chroma}:${ink}`;
    const tones = look && nextKey === key ? look.tones : toneTable(Number.isFinite(hue) ? hue : 55, Number.isFinite(chroma) ? chroma : 0.14, ink);
    key = nextKey;
    look = { tones, font };
    return look;
  };
}

export const toneIndex = (x: number) => Math.max(0, Math.min(255, Math.round(x * 255))) * 3;

/** A CSS colour for tone `x` (0..1). */
export function toneColour(table: ToneTable, x: number, alpha = 1) {
  const index = toneIndex(x);
  return `rgba(${table[index]},${table[index + 1]},${table[index + 2]},${alpha})`;
}

/** Write tone `x` into the RGBA pixel starting at byte `at`. */
export function putTone(data: Uint8ClampedArray, at: number, table: ToneTable, x: number, alpha = 255) {
  const index = toneIndex(x);
  data[at] = table[index]; data[at + 1] = table[index + 1]; data[at + 2] = table[index + 2]; data[at + 3] = alpha;
}

/** A small offscreen canvas a drawing fills pixel by pixel and then scales onto its visible canvas. */
export class PixelBuffer {
  readonly data: Uint8ClampedArray;
  private readonly canvas: HTMLCanvasElement;
  private readonly context: CanvasRenderingContext2D | null;
  private readonly image: ImageData | null;

  constructor(readonly width: number, readonly height: number) {
    this.canvas = document.createElement("canvas");
    this.canvas.width = width; this.canvas.height = height;
    this.context = this.canvas.getContext("2d");
    this.image = this.context?.createImageData(width, height) ?? null;
    this.data = this.image?.data ?? new Uint8ClampedArray(width * height * 4);
  }

  /** Scale the buffer over the whole of `target` (`width` x `height` in its own units). Pixelated keeps hard edges. */
  blit(target: CanvasRenderingContext2D, width: number, height: number, pixelated: boolean) {
    if (!this.context || !this.image) return;
    this.context.putImageData(this.image, 0, 0);
    target.imageSmoothingEnabled = !pixelated;
    target.clearRect(0, 0, width, height);
    target.drawImage(this.canvas, 0, 0, width, height);
  }
}
