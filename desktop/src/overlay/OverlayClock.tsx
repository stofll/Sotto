import { useEffect, useRef } from "react";
import { binaryDigits, CLOCK_CELL, clockCells, clockSize, GLYPH_ROWS, segmentLayout, type ClockDraw } from "./clockPainters";
import { paletteLook, toneColour } from "./pixelDither";

type Props = {
  draw: ClockDraw;
  /** The clock text, `MM:SS` or the loading dashes. */
  text: string;
  /** The limit countdown: drawn in the warning colour. */
  warn: boolean;
};

/**
 * The timer drawn as seven segments, LED dots, a grid of characters or
 * binary dots. A clock changes once a second, so it repaints when its text
 * does (and whenever the scene renders again, which picks up a palette
 * change) instead of running on the frame clock.
 */
export function OverlayClock({ draw, text, warn }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [width, height] = clockSize(draw, text);

  useEffect(() => {
    const canvas = canvasRef.current, context = canvas?.getContext("2d");
    if (!canvas || !context) return;
    const ratio = window.devicePixelRatio || 1;
    const pixelWidth = Math.round(width * ratio), pixelHeight = Math.round(height * ratio);
    if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) { canvas.width = pixelWidth; canvas.height = pixelHeight; }
    const { tones, font } = paletteLook(canvas)(performance.now());
    const alarm = warn ? getComputedStyle(canvas).getPropertyValue("--warn").trim() || "#fbbf24" : null;
    // A lit mark and a dim one: the same ink at two strengths.
    const ink = (lit: boolean) => {
      context.globalAlpha = 1;
      if (alarm) { context.globalAlpha = lit ? 1 : 0.16; return alarm; }
      return lit ? toneColour(tones, 0.95) : toneColour(tones, 0.5, 0.15);
    };
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, width, height);
    const margin = CLOCK_CELL.margin;
    if (draw === "seg") {
      const { glyphs, height: glyphHeight } = segmentLayout(text);
      const top = (height - glyphHeight) / 2;
      // The colon blinks with the seconds, which is what the text already counts.
      const colonOn = Number(text.slice(-1)) % 2 === 0;
      for (const glyph of glyphs) {
        if (glyph.kind === "colon") {
          context.fillStyle = ink(colonOn);
          for (const [x, y, w, h] of glyph.dots) context.fillRect(margin + x, top + y, w, h);
          continue;
        }
        for (const [name, [x, y, w, h]] of Object.entries(glyph.segments)) {
          context.fillStyle = ink(glyph.on.includes(name));
          context.fillRect(margin + x, top + y, w, h);
        }
      }
    } else if (draw === "led") {
      const { cells, columns } = clockCells(text);
      const pitch = CLOCK_CELL.led, left = (width - columns * pitch) / 2, top = (height - GLYPH_ROWS * pitch) / 2;
      for (const lit of [false, true]) {
        context.fillStyle = ink(lit);
        context.beginPath();
        for (const cell of cells) {
          if (cell.lit !== lit) continue;
          const x = left + (cell.column + 0.5) * pitch, y = top + (cell.row + 0.5) * pitch;
          context.moveTo(x + 1.6, y); context.arc(x, y, 1.6, 0, Math.PI * 2);
        }
        context.fill();
      }
    } else if (draw === "glyph") {
      const { cells, columns } = clockCells(text);
      const stepX = CLOCK_CELL.glyphWidth, stepY = CLOCK_CELL.glyphHeight;
      const left = (width - columns * stepX) / 2, top = (height - GLYPH_ROWS * stepY) / 2;
      context.font = font.replace(/\d+(\.\d+)?px/, "7px").replace(/^400/, "600");
      context.textAlign = "center"; context.textBaseline = "middle";
      for (const cell of cells) {
        context.fillStyle = ink(cell.lit);
        context.fillText(cell.lit ? "#" : "·", left + (cell.column + 0.5) * stepX, top + (cell.row + 0.5) * stepY);
      }
    } else {
      const digits = binaryDigits(text), pitch = CLOCK_CELL.binary;
      const left = (width - (digits.length * pitch + CLOCK_CELL.binaryGap)) / 2 + pitch / 2, top = (height - 4 * pitch) / 2 + pitch / 2;
      digits.forEach((bits, index) => {
        // The gap sits between the minutes and the seconds.
        const x = left + index * pitch + (index >= digits.length / 2 ? CLOCK_CELL.binaryGap : 0);
        bits.forEach((lit, bit) => {
          context.fillStyle = ink(lit);
          context.beginPath(); context.arc(x, top + bit * pitch, 2.2, 0, Math.PI * 2); context.fill();
        });
      });
    }
    context.globalAlpha = 1;
  });

  return <canvas ref={canvasRef} className="ovs-clock" style={{ width, height }} aria-hidden="true"/>;
}
