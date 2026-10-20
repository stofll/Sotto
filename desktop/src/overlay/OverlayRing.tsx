import { useEffect, useRef } from "react";
import { frameRunner, onFrame, prefersReducedMotion } from "./frameClock";
import { audioLevelSource, type LevelSource } from "./levelSource";
import { RING_TICKS, ringTicks } from "./levelPainters";
import { paletteLook, toneColour } from "./pixelDither";
import { RING_HOLE } from "./shellShapes";

const HISTORY = 96;
const FRAME_MS = 30;
const REDUCED_FRAME_MS = 250;

type Props = {
  /** The side of the ring shell, px. */
  size: number;
  mode: "recording" | "processing" | "idle";
  /** False while hidden or in a thumbnail: the rim is painted once and rests. */
  live: boolean;
  source?: LevelSource;
  /** The processing speed, as a factor of the normal pace. */
  pace?: number;
  /** A hairline round the hole, for shells that have a stroke. */
  hairline?: boolean;
  /** Square-ended ticks, for the sharp style. */
  sharp?: boolean;
};

/** The ring shell's rim: the voice as radial ticks, a comet while processing. */
export function OverlayRing({ size, mode, live, source = audioLevelSource, pace = 1, hairline = false, sharp = false }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current, context = canvas?.getContext("2d");
    if (!canvas || !context) return;
    const reduced = prefersReducedMotion();
    const levels: number[] = Array(HISTORY).fill(0);
    const readLook = paletteLook(canvas);
    const ratio = window.devicePixelRatio || 1;
    const pixels = Math.round(size * ratio);
    if (canvas.width !== pixels || canvas.height !== pixels) { canvas.width = pixels; canvas.height = pixels; }
    const centre = size / 2, outer = centre - 1, inner = outer * RING_HOLE;
    const near = inner + 4, reach = outer - inner - 9;
    let painted = -Infinity;

    const paint = (now: number) => {
      const { tones } = readLook(now);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      context.clearRect(0, 0, size, size);
      if (hairline) {
        context.lineWidth = 1; context.strokeStyle = toneColour(tones, 0.6, 0.35);
        context.beginPath(); context.arc(centre, centre, inner + 0.5, 0, Math.PI * 2); context.stroke();
      }
      context.lineWidth = Math.max(1.5, size / 44); context.lineCap = sharp ? "butt" : "round";
      ringTicks(mode, levels, (now / 1000) * pace).forEach((amplitude, tick) => {
        const angle = -Math.PI / 2 + (tick / RING_TICKS) * Math.PI * 2;
        const from = near, to = near + 1.5 + amplitude * reach;
        context.strokeStyle = toneColour(tones, 0.45 + 0.5 * amplitude, 0.4 + 0.6 * amplitude);
        context.beginPath();
        context.moveTo(centre + Math.cos(angle) * from, centre + Math.sin(angle) * from);
        context.lineTo(centre + Math.cos(angle) * to, centre + Math.sin(angle) * to);
        context.stroke();
      });
    };
    paint(performance.now());
    if (!live || mode === "idle") return;

    const throttled = (now: number) => {
      if (now - painted < (reduced ? REDUCED_FRAME_MS : FRAME_MS)) return;
      painted = now;
      paint(now);
    };
    if (mode === "processing") return onFrame(throttled);
    // Recording paints while readings keep arriving, so a quiet or hidden overlay costs nothing.
    const runner = frameRunner(throttled);
    const stop = source((reading) => {
      levels.push(reading);
      levels.shift();
      runner.wake();
    });
    return () => { stop(); runner.stop(); };
  }, [size, mode, live, source, pace, hairline, sharp]);

  return <canvas ref={canvasRef} className="ovs-ring-rim" style={{ width: size, height: size }} aria-hidden="true"/>;
}
