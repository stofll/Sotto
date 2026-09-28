import { useEffect, useRef, type RefObject } from "react";
import { frameRunner, onFrame } from "./frameClock";
import { audioLevelSource, type LevelSource } from "./levelSource";
import { brightnessStep, matrixGrid, paintProcess, paintSpeech, speechState, type MatrixDensity, type MatrixGrid, type MatrixProcess, type MatrixSpeech } from "./dotMatrix";

const HISTORY = 96;
// Dot diameter as a share of the cell pitch: close enough to read as one
// surface, far enough apart that every dot stays a dot.
const DOT = 0.64;

type Props = {
  mode: "speech" | "process";
  /** A square grid of the given size in px; without it the grid fills its box as a row. */
  size?: number;
  density: MatrixDensity;
  speech: MatrixSpeech;
  process: MatrixProcess;
  surfaceRef?: RefObject<HTMLDivElement | null>;
  source?: LevelSource;
  /** Square dots instead of round ones, for the sharp style. */
  sharp?: boolean;
  /** How many times faster than normal the processing pattern runs. */
  pace?: number;
};

export function OverlayMatrix({ mode, size, density, speech, process, surfaceRef, source = audioLevelSource, sharp = false, pace = 1 }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return;
    let grid: MatrixGrid | null = null;
    let cells = new Float32Array(0);
    let colours = { mid: "", top: "", idle: "" };
    let coloursAt = -Infinity;

    const fit = () => {
      const ratio = window.devicePixelRatio || 1;
      const width = size ?? canvas.clientWidth, height = size ?? canvas.clientHeight;
      if (!width || !height) return false;
      const pixelWidth = Math.round(width * ratio), pixelHeight = Math.round(height * ratio);
      if (canvas.width !== pixelWidth || canvas.height !== pixelHeight || !grid) {
        canvas.width = pixelWidth; canvas.height = pixelHeight;
        grid = matrixGrid(width, height, density, size !== undefined);
        cells = new Float32Array(grid.cols * grid.rows);
      }
      return true;
    };
    const draw = (now: number) => {
      if (!grid) return;
      // The palette can change while the overlay is open; re-read it now and then, not per frame.
      if (now - coloursAt > 1000) {
        const style = getComputedStyle(canvas);
        colours = {
          mid: style.getPropertyValue("--overlay-wave-mid").trim() || "#ff8a3d", top: style.getPropertyValue("--overlay-wave-top").trim() || "#ffc27a",
          idle: style.getPropertyValue("--ovs-idle").trim() || "rgba(255, 255, 255, 0.09)",
        };
        coloursAt = now;
      }
      const ratio = canvas.width / (size ?? canvas.clientWidth);
      const radius = grid.pitch * DOT / 2;
      const paths = [new Path2D(), new Path2D(), new Path2D(), new Path2D()];
      for (let index = 0; index < cells.length; index++) {
        const x = grid.offsetX + (index % grid.cols + 0.5) * grid.pitch;
        const y = grid.offsetY + (Math.floor(index / grid.cols) + 0.5) * grid.pitch;
        const path = paths[brightnessStep(cells[index])];
        if (sharp) path.rect(x - radius, y - radius, radius * 2, radius * 2);
        else { path.moveTo(x + radius, y); path.arc(x, y, radius, 0, Math.PI * 2); }
      }
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      context.clearRect(0, 0, canvas.width, canvas.height);
      context.globalAlpha = 1;
      context.fillStyle = colours.idle;
      context.fill(paths[0]);
      context.fillStyle = colours.mid;
      context.globalAlpha = 0.45; context.fill(paths[1]);
      context.globalAlpha = 0.75; context.fill(paths[2]);
      context.globalAlpha = 1; context.fillStyle = colours.top; context.fill(paths[3]);
    };

    const observer = size === undefined ? new ResizeObserver(() => { fit(); draw(performance.now()); }) : null;
    observer?.observe(canvas);
    fit();

    if (mode === "process") {
      const started = performance.now();
      const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      const tick = (now: number) => {
        if (!fit() || !grid) return;
        // Reduced motion keeps the pattern but moves it in visible steps, four a second.
        paintProcess(grid, cells, process, (reduced ? Math.floor((now - started) / 250) * 250 : now - started) / 1000 * pace);
        draw(now);
      };
      let stopFrames = () => {}, timer = 0;
      if (reduced) { tick(performance.now()); timer = window.setInterval(() => tick(performance.now()), 250); }
      else stopFrames = onFrame(tick);
      return () => { stopFrames(); window.clearInterval(timer); observer?.disconnect(); };
    }

    const levels: number[] = [];
    const state = speechState();
    let energy = 0.08;
    const surface = surfaceRef?.current;
    surface?.style.setProperty("--overlay-energy", String(energy));
    draw(performance.now());
    // Painted on the shared ~60 fps clock so rings and fades move between readings.
    const runner = frameRunner((now) => {
      if (!fit() || !grid) return;
      paintSpeech(grid, cells, speech, levels, now, state);
      draw(now);
    });
    const stop = source((sample) => {
      levels.push(sample);
      if (levels.length > HISTORY) levels.shift();
      energy = energy * 0.78 + Math.sqrt(sample) * 0.22;
      surface?.style.setProperty("--overlay-energy", String(Math.max(0.08, energy)));
      runner.wake();
    });
    return () => { stop(); runner.stop(); observer?.disconnect(); surface?.style.removeProperty("--overlay-energy"); };
  }, [mode, size, density, speech, process, surfaceRef, source, sharp, pace]);

  return <canvas ref={canvasRef} className={`overlay-matrix${size === undefined ? " overlay-matrix--row" : ""}`}
    style={size === undefined ? undefined : { width: size, height: size }} aria-hidden="true"/>;
}
