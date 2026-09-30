// One requestAnimationFrame loop for every animated part of a window, held to
// about 60 frames a second. A 120 or 144 Hz display would otherwise double the
// paint work for motion the eye does not tell apart. The loop runs only while
// something is subscribed, so an idle window does no work at all.

export type FrameTick = (now: number, seconds: number) => void;

const FRAME_MS = 1000 / 60;
// A frame that lands a hair early still runs; otherwise a 60 Hz display would drop to 30.
const SLACK_MS = 2;

const ticks = new Set<FrameTick>();
let request = 0;
// `slot` is the 60 fps grid the frames are held to; `painted` is when the last one ran.
let slot = 0;
let painted = 0;

function loop(now: number) {
  request = requestAnimationFrame(loop);
  const elapsed = now - slot;
  if (elapsed < FRAME_MS - SLACK_MS) return;
  // Carry the remainder over: a 144 Hz frame never lands on the 60 fps grid,
  // and restarting the count at each frame would settle at 48 fps.
  slot = elapsed < FRAME_MS * 2 ? now - (elapsed % FRAME_MS) : now;
  // Exponential easing needs wall time, including delayed frames, because
  // frameRunner also ends its settling period against the wall clock.
  const seconds = (now - painted) / 1000;
  painted = now;
  for (const tick of ticks) tick(now, seconds);
}

/** Call `tick` on every frame until the returned function is called. */
export function onFrame(tick: FrameTick): () => void {
  ticks.add(tick);
  if (!request) { slot = painted = performance.now(); request = requestAnimationFrame(loop); }
  return () => {
    ticks.delete(tick);
    if (!ticks.size && request) { cancelAnimationFrame(request); request = 0; }
  };
}

/** How far a value eased with time constant `tau` moves toward its target in `seconds`. */
export const easeStep = (seconds: number, tau: number) => 1 - Math.exp(-seconds / tau);

/**
 * Frames for a part driven by level readings: `wake` on each reading keeps
 * `tick` running, and it stops `linger` ms after the last one. A thumbnail
 * that gets a single still reading paints briefly and then costs nothing;
 * a live recording, which sends readings even in silence, animates throughout.
 */
export function frameRunner(tick: FrameTick, linger = 600) {
  let stop: (() => void) | null = null;
  let until = 0;
  const run: FrameTick = (now, seconds) => {
    tick(now, seconds);
    if (now > until && stop) { stop(); stop = null; }
  };
  return {
    wake() { until = performance.now() + linger; stop ??= onFrame(run); },
    stop() { stop?.(); stop = null; },
  };
}
