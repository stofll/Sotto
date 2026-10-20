import { useEffect, useRef } from "react";
import { frameRunner } from "./frameClock";
import { audioLevelSource, type LevelSource } from "./levelSource";
import {
  COLUMN_ROWS, bufferSize, columnGlyph, glitchState, halftoneDots, lavaScratch, paintGlitch, paintLava, paintOrb, paintStatic, paintTide, tearGlitch,
  type GlitchState, type LavaScratch, type PixelLevel,
} from "./levelPainters";
import { PixelBuffer, historyAt, paletteLook, toneColour } from "./pixelDither";

const HISTORY = 96;
// Painting at the level readings' own rate is enough for these drawings.
const FRAME_MS = 30;
const REDUCED_FRAME_MS = 250;

export type PixelLevelDraw = PixelLevel | "halftone" | "cols";

type Props = {
  draw: PixelLevelDraw;
  /** A square of the given size in px; without it the canvas fills its box. */
  size?: number;
  source?: LevelSource;
  /** Square dots instead of round ones, for the sharp style. */
  sharp?: boolean;
};

/** Level drawings built from a low-resolution buffer, a dot screen or a few rows of characters. */
export function OverlayPixelLevel({ draw, size, source = audioLevelSource, sharp = false }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const levels: number[] = Array(HISTORY).fill(0);
    let level = 0, painted = -Infinity;
    const readLook = paletteLook(canvas);
    let buffer: PixelBuffer | null = null, glitch: GlitchState | null = null, lava: LavaScratch | null = null;
    let width = 0, height = 0, ratio = 1, glyphWidth = 0, glyphFont = "";

    const fit = () => {
      width = size ?? canvas.clientWidth; height = size ?? canvas.clientHeight;
      if (!width || !height) return false;
      ratio = window.devicePixelRatio || 1;
      const pixelWidth = Math.round(width * ratio), pixelHeight = Math.round(height * ratio);
      if (canvas.width !== pixelWidth || canvas.height !== pixelHeight || (draw !== "halftone" && draw !== "cols" && !buffer)) {
        canvas.width = pixelWidth; canvas.height = pixelHeight;
        if (draw !== "halftone" && draw !== "cols") {
          const [bufferWidth, bufferHeight] = bufferSize(draw, width, height);
          buffer = new PixelBuffer(bufferWidth, bufferHeight);
          glitch = glitchState();
          lava = lavaScratch(bufferWidth, bufferHeight);
        }
      }
      return true;
    };
    const paint = (now: number) => {
      if (!fit()) return;
      const { tones, font } = readLook(now);
      const seconds = now / 1000;
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      if (draw === "halftone") {
        context.clearRect(0, 0, width, height);
        const paths = [new Path2D(), new Path2D(), new Path2D()];
        for (const dot of halftoneDots(width, height, levels)) {
          const path = paths[dot.tone];
          if (sharp) path.rect(dot.x - dot.radius, dot.y - dot.radius, dot.radius * 2, dot.radius * 2);
          else { path.moveTo(dot.x + dot.radius, dot.y); path.arc(dot.x, dot.y, dot.radius, 0, Math.PI * 2); }
        }
        [0.45, 0.72, 0.95].forEach((tone, index) => { context.fillStyle = toneColour(tones, tone); context.fill(paths[index]); });
        return;
      }
      if (draw === "cols") {
        context.clearRect(0, 0, width, height);
        context.font = font; context.textAlign = "center"; context.textBaseline = "middle";
        if (font !== glyphFont) { glyphFont = font; glyphWidth = context.measureText("M").width + 1; }
        const columns = Math.max(1, Math.floor(width / glyphWidth)), rowHeight = height / COLUMN_ROWS;
        for (let column = 0; column < columns; column++) {
          const amplitude = Math.sqrt(historyAt(levels, (columns - 1 - column) * 1.6));
          for (let row = 0; row < COLUMN_ROWS; row++) {
            const glyph = columnGlyph(amplitude, row), x = (column + 0.5) * glyphWidth, y = (row + 0.5) * rowHeight;
            context.fillStyle = toneColour(tones, glyph ? glyph.tone : 0.5, glyph ? 1 : 0.18);
            context.fillText(glyph ? glyph.char : "·", x, y);
          }
        }
        return;
      }
      if (!buffer || !glitch || !lava) return;
      const { data, width: bufferWidth, height: bufferHeight } = buffer;
      switch (draw) {
        case "dorb": paintOrb(data, bufferWidth, level, seconds, tones); break;
        case "tide": paintTide(data, bufferWidth, bufferHeight, levels, tones); break;
        case "static": paintStatic(data, bufferWidth, bufferHeight, levels, tones); break;
        case "glitch": tearGlitch(glitch, seconds, level, reduced); paintGlitch(data, bufferWidth, bufferHeight, levels, level, glitch, tones); break;
        case "lava": paintLava(data, bufferWidth, bufferHeight, levels, seconds, lava, tones); break;
      }
      buffer.blit(context, width, height, true);
    };

    const observer = size === undefined ? new ResizeObserver(() => paint(performance.now())) : null;
    observer?.observe(canvas);
    paint(performance.now());
    // Painted on the shared clock while readings keep arriving; a quiet or hidden overlay costs nothing.
    const runner = frameRunner((now) => {
      if (now - painted < (reduced ? REDUCED_FRAME_MS : FRAME_MS)) return;
      painted = now;
      paint(now);
    });
    const stop = source((reading) => {
      // A quick attack and a slower release, so the drawings swell with a syllable and settle after it.
      level += (reading - level) * (reading > level ? 0.55 : 0.25);
      levels.push(level);
      levels.shift();
      runner.wake();
    });
    return () => { stop(); runner.stop(); observer?.disconnect(); };
  }, [draw, size, source, sharp]);

  return <canvas ref={canvasRef} className={`ovs-px${size === undefined ? " ovs-px--row" : ""}`}
    style={size === undefined ? undefined : { width: size, height: size }} aria-hidden="true"/>;
}
