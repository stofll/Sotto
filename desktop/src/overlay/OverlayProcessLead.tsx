import { useEffect, useRef } from "react";
import { onFrame } from "./frameClock";
import { barCells, paintWaves, scanX, wavefrontDots } from "./processPainters";
import { PixelBuffer, paletteLook, toneColour } from "./pixelDither";

const FRAME_MS = 33;
const REDUCED_FRAME_MS = 250;

export type ProcessCanvasDraw = "waves" | "scan" | "bar" | "halftone";

type Props = {
  draw: ProcessCanvasDraw;
  /** CSS px. */
  width: number;
  height: number;
  /** How many times faster than normal the drawing runs. */
  pace?: number;
  /** Square dots instead of round ones, for the sharp style. */
  sharp?: boolean;
};

/** The processing drawings that paint on a canvas: rings, a scan line with a phosphor trail, a bracketed bar and a dot wave. */
export function OverlayProcessLead({ draw, width, height, pace = 1, sharp = false }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current, context = canvas?.getContext("2d");
    if (!canvas || !context) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * ratio); canvas.height = Math.round(height * ratio);
    const readLook = paletteLook(canvas);
    const buffer = draw === "waves" ? new PixelBuffer(Math.max(2, Math.round(width / 2)), Math.max(2, Math.round(height / 2))) : null;
    let painted = -Infinity, glyphWidth = 0, glyphFont = "";
    const started = performance.now();

    const paint = (now: number, elapsed: number) => {
      const { tones, font } = readLook(now), seconds = (now - started) / 1000 * pace;
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      if (draw === "waves" && buffer) {
        paintWaves(buffer.data, buffer.width, buffer.height, seconds, tones);
        buffer.blit(context, width, height, true);
      } else if (draw === "scan") {
        // The canvas keeps its previous frame, so erasing a share of it each time leaves a phosphor trail.
        context.globalCompositeOperation = "destination-out";
        context.fillStyle = `rgba(0,0,0,${1 - Math.pow(0.84, Math.min(6, elapsed / 16.7))})`;
        context.fillRect(0, 0, width, height);
        context.globalCompositeOperation = "source-over";
        const x = scanX(seconds, width);
        context.fillStyle = toneColour(tones, 0.75, 0.35); context.fillRect(x - 5, 0, 12, height);
        context.fillStyle = toneColour(tones, 0.98); context.fillRect(x, 0, 2, height);
        context.globalCompositeOperation = "destination-out";
        context.fillStyle = "rgba(0,0,0,0.55)";
        for (let y = 0; y < height; y += 3) context.fillRect(0, y, width, 1);
        context.globalCompositeOperation = "source-over";
      } else if (draw === "bar") {
        context.clearRect(0, 0, width, height);
        context.font = font; context.textAlign = "center"; context.textBaseline = "middle";
        if (font !== glyphFont) { glyphFont = font; glyphWidth = context.measureText("#").width + 0.5; }
        const columns = Math.floor(width / glyphWidth), offset = (width - columns * glyphWidth) / 2;
        barCells(columns, seconds).forEach((cell, column) => {
          context.fillStyle = toneColour(tones, cell.tone, cell.alpha);
          context.fillText(cell.char, offset + (column + 0.5) * glyphWidth, height / 2);
        });
      } else if (draw === "halftone") {
        context.clearRect(0, 0, width, height);
        const paths = [new Path2D(), new Path2D(), new Path2D()];
        for (const dot of wavefrontDots(width, height, seconds)) {
          const path = paths[dot.tone];
          if (sharp) path.rect(dot.x - dot.radius, dot.y - dot.radius, dot.radius * 2, dot.radius * 2);
          else { path.moveTo(dot.x + dot.radius, dot.y); path.arc(dot.x, dot.y, dot.radius, 0, Math.PI * 2); }
        }
        [0.45, 0.72, 0.95].forEach((tone, index) => { context.fillStyle = toneColour(tones, tone); context.fill(paths[index]); });
      }
    };

    paint(performance.now(), 16.7);
    // The component exists only while the overlay is processing, so the shared clock runs only then.
    const stop = onFrame((now) => {
      const elapsed = now - painted;
      if (elapsed < (reduced ? REDUCED_FRAME_MS : FRAME_MS)) return;
      painted = now;
      paint(now, elapsed);
    });
    return stop;
  }, [draw, width, height, pace, sharp]);

  return <canvas ref={canvasRef} className="ovs-pl" style={{ width, height }} aria-hidden="true"/>;
}
