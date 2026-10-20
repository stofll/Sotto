import { useEffect, useRef, type RefObject } from "react";
import { easeStep, onFrame, prefersReducedMotion } from "./frameClock";
import { audioLevelSource, type LevelSource } from "./levelSource";
import { fillGrain, idleEnergy, paintSmoke, smokeBufferSize, smokeRate, type SmokeField, type SmokeFrame } from "./fillPainters";
import { PixelBuffer, paletteLook } from "./pixelDither";

const SMOKE_FRAME_MS = 33;
const GRAIN_FRAME_MS = 83;
const REDUCED_FRAME_MS = 250;
const GRAIN_TILE = 96;
/** How much bigger than a CSS px a grain speck is. */
const GRAIN_SCALE = 1.5;

type Props = {
  /** Whether the scene is moving: recording or processing. Otherwise the picture is painted once and stays. */
  live: boolean;
  /** False while the overlay is hidden between sessions. */
  shown?: boolean;
};

/**
 * One smoke field shown by several canvases, each painting the part under it:
 * the split's two capsules. The `lead` canvas moves the field on; the others
 * only read it, so the smoke runs at one speed and lines up across the gap.
 */
export type SharedSmoke = { frame: RefObject<SmokeFrame>; span: RefObject<HTMLElement | null>; lead: boolean };

const newSmokeFrame = (): SmokeFrame => ({ time: 0, drift: 0, energy: 0.18 });

/** The smoke fill: fbm smoke in a low-resolution buffer behind the content, brighter and faster with the voice. */
export function OverlaySmoke({ source = audioLevelSource, live, recording, shown = true, shared }: Props & { source?: LevelSource; recording: boolean; shared?: SharedSmoke }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const level = useRef(0);
  const own = useRef<SmokeFrame>(newSmokeFrame());
  const smoke = shared?.frame ?? own;
  const leads = !shared || shared.lead;

  useEffect(() => source((reading) => { level.current += (reading - level.current) * (reading > level.current ? 0.55 : 0.25); }), [source]);

  useEffect(() => {
    const canvas = canvasRef.current, context = canvas?.getContext("2d");
    if (!canvas || !context || !shown) return;
    const reduced = prefersReducedMotion();
    const readLook = paletteLook(canvas);
    let buffer: PixelBuffer | null = null, last = -Infinity;

    const paint = (now: number, seconds: number) => {
      const width = canvas.clientWidth, height = canvas.clientHeight;
      if (!width || !height) return;
      if (canvas.width !== Math.round(width) || canvas.height !== Math.round(height)) { canvas.width = Math.round(width); canvas.height = Math.round(height); }
      const [bufferWidth, bufferHeight] = smokeBufferSize(width, height);
      if (!buffer || buffer.width !== bufferWidth || buffer.height !== bufferHeight) buffer = new PixelBuffer(bufferWidth, bufferHeight);
      const state = smoke.current;
      if (leads) {
        // Without a voice the energy settles to a faint swell and the smoke drifts sideways.
        const target = recording ? Math.sqrt(level.current) : idleEnergy(state.drift);
        state.energy += (target - state.energy) * easeStep(seconds, recording ? 0.3 : 0.6);
        state.time += seconds * smokeRate(recording, state.energy);
        if (!recording) state.drift += seconds * 0.25;
      }
      paintSmoke(buffer.data, buffer.width, buffer.height, state, readLook(now).tones, fieldOf(canvas, buffer.width, shared?.span.current ?? null));
      buffer.blit(context, width, height, false);
    };

    paint(performance.now(), 0);
    if (!live) return;
    const observer = new ResizeObserver(() => paint(performance.now(), 0));
    observer.observe(canvas);
    const stop = onFrame((now) => {
      if (now - last < (reduced ? REDUCED_FRAME_MS : SMOKE_FRAME_MS)) return;
      // A long gap (the tab was hidden, the window was concealed) must not leap the smoke ahead.
      const seconds = Math.min(0.1, (now - last) / 1000);
      last = now;
      paint(now, seconds);
    });
    return () => { stop(); observer.disconnect(); };
  }, [live, recording, shown, smoke, leads, shared?.span]);

  return <canvas ref={canvasRef} className="ovs-fill-smoke" aria-hidden="true"/>;
}

/** The part of `span` under `canvas`, in the canvas buffer's pixels; the whole buffer without a span. Rects are
 *  compared as drawn, so a scaled thumbnail lines up too. */
function fieldOf(canvas: HTMLCanvasElement, bufferWidth: number, span: HTMLElement | null): SmokeField {
  if (!span) return { x: 0, width: bufferWidth };
  const own = canvas.getBoundingClientRect(), whole = span.getBoundingClientRect();
  if (!own.width || !whole.width) return { x: 0, width: bufferWidth };
  const unit = bufferWidth / own.width;
  return { x: (own.left - whole.left) * unit, width: whole.width * unit };
}

/** The grain texture: a small tile of noise, shifted at random a few times a second so the film seems to crawl. */
export function OverlayGrain({ live, shown = true }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current, context = canvas?.getContext("2d");
    if (!canvas || !context || !shown) return;
    const tile = document.createElement("canvas");
    tile.width = tile.height = GRAIN_TILE;
    const tileContext = tile.getContext("2d");
    const image = tileContext?.createImageData(GRAIN_TILE, GRAIN_TILE);
    if (!tileContext || !image) return;
    fillGrain(image.data);
    tileContext.putImageData(image, 0, 0);
    const pattern = context.createPattern(tile, "repeat");
    if (!pattern) return;
    let last = -Infinity;

    const paint = () => {
      const width = canvas.clientWidth, height = canvas.clientHeight;
      if (!width || !height) return;
      if (canvas.width !== Math.round(width) || canvas.height !== Math.round(height)) { canvas.width = Math.round(width); canvas.height = Math.round(height); }
      context.setTransform(1, 0, 0, 1, 0, 0);
      context.clearRect(0, 0, width, height);
      context.setTransform(GRAIN_SCALE, 0, 0, GRAIN_SCALE, -Math.random() * GRAIN_TILE, -Math.random() * GRAIN_TILE);
      context.fillStyle = pattern;
      context.fillRect(0, 0, width / GRAIN_SCALE + GRAIN_TILE, height / GRAIN_SCALE + GRAIN_TILE);
    };
    paint();
    const observer = new ResizeObserver(paint);
    observer.observe(canvas);
    // With reduced motion the grain is one still pattern.
    const stop = live && !prefersReducedMotion() ? onFrame((now) => {
      if (now - last < GRAIN_FRAME_MS) return;
      last = now;
      paint();
    }) : null;
    return () => { stop?.(); observer.disconnect(); };
  }, [live, shown]);

  return <canvas ref={canvasRef} className="ovs-fill-grain" aria-hidden="true"/>;
}
