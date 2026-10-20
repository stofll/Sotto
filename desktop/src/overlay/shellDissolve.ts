// The "inserted" note's dissolving exit: the scene leaves through a dither
// mask that eats it away in coarse cells instead of fading it.

import { useEffect, type RefObject } from "react";
import { onFrame, prefersReducedMotion } from "./frameClock";
import { bayerThreshold } from "./pixelDither";

/** How long the dissolve runs. The native side conceals the window `LEAVE_DURATION` (220 ms, overlay.rs) after "overlay-leaving", and the fade it replaces takes 180 ms. */
export const DISSOLVE_MS = 180;
const STEPS = 9;
/** One mask cell covers this many CSS px. */
const CELL = 3;

/** A hash of two small integers in [0, 1), so neighbouring cells do not dissolve in a visible pattern. */
function cellNoise(x: number, y: number) {
  let h = Math.imul(x, 374761393) + Math.imul(y, 668265263);
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}

/** When a cell goes, in 0..1: a Bayer pattern mixed with noise, so the edge is ordered but not regular. */
export const dissolveThreshold = (x: number, y: number) => 0.6 * bayerThreshold(x, y) + 0.4 * cellNoise(x >> 1, y >> 1);

/** Whether a cell is still shown at `progress` (0 = whole, 1 = gone). Anything shown at a later time was shown earlier. */
export const dissolveShows = (x: number, y: number, progress: number) => dissolveThreshold(x, y) > progress * 1.05;

/** The mask's step for `elapsed` ms into the dissolve. */
export const dissolveStep = (elapsed: number) => Math.max(0, Math.min(STEPS, Math.floor(elapsed / DISSOLVE_MS * STEPS)));

/** The mask as a PNG data URL, `width` x `height` CSS px, opaque where the scene is still shown. */
function maskImage(width: number, height: number, progress: number) {
  const columns = Math.ceil(width / CELL), rows = Math.ceil(height / CELL);
  const small = document.createElement("canvas");
  small.width = columns; small.height = rows;
  const context = small.getContext("2d");
  if (!context) return "";
  const image = context.createImageData(columns, rows);
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < columns; x++) {
      const at = (y * columns + x) * 4;
      image.data[at + 3] = dissolveShows(x, y, progress) ? 255 : 0;
    }
  }
  context.putImageData(image, 0, 0);
  // Scaled up without smoothing, or the cells would blur into one another.
  const full = document.createElement("canvas");
  full.width = columns * CELL; full.height = rows * CELL;
  const target = full.getContext("2d");
  if (!target) return "";
  target.imageSmoothingEnabled = false;
  target.drawImage(small, 0, 0, full.width, full.height);
  return `url(${full.toDataURL()})`;
}

/** Whether the exit may dissolve: stepping masks over 180 ms is exactly the motion a reduced-motion setting asks to avoid, so it fades instead. */
export const dissolveAllowed = () => !prefersReducedMotion();

/** While `active`, steps `node`'s mask from whole to gone over `DISSOLVE_MS`; the mask is removed when it turns inactive. */
export function useShellDissolve(ref: RefObject<HTMLElement | null>, active: boolean) {
  useEffect(() => {
    const node = ref.current;
    if (!node || !active) return;
    const { offsetWidth: width, offsetHeight: height } = node;
    if (!width || !height) return;
    const cache = new Map<number, string>();
    let started = -1, shown = -1;
    const apply = (step: number) => {
      if (step === shown) return;
      shown = step;
      let image = cache.get(step);
      if (image === undefined) cache.set(step, image = maskImage(width, height, step / STEPS));
      node.style.maskImage = image; node.style.webkitMaskImage = image;
      node.style.maskSize = "100% 100%"; node.style.webkitMaskSize = "100% 100%";
    };
    apply(0);
    const stop = onFrame((now) => {
      if (started < 0) started = now;
      apply(dissolveStep(now - started));
    });
    return () => {
      stop();
      node.style.maskImage = ""; node.style.webkitMaskImage = "";
      node.style.maskSize = ""; node.style.webkitMaskSize = "";
    };
  }, [ref, active]);
}
