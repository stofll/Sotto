// Outlines of the shells that are not a rounded rectangle. The overlay window
// is a transparent rectangle; a shell cuts its shape out with `clip-path`,
// which the browser also uses for hit-testing, so the ring's hole does not
// count as the overlay for hover (see isOverlayBody).

/** The ring's hole as a share of its outer radius. */
export const RING_HOLE = 0.56;

const round = (value: number) => Math.round(value * 100) / 100;

/** A donut: the outer circle of a square shell with the hole cut out of it, as a CSS `clip-path`. */
export function ringClip(size: number, hole = RING_HOLE): string {
  const outer = size / 2, inner = outer * hole;
  const circle = (radius: number) => `M${round(outer - radius)} ${round(outer)}a${round(radius)} ${round(radius)} 0 1 0 ${round(radius * 2)} 0a${round(radius)} ${round(radius)} 0 1 0 ${round(-radius * 2)} 0Z`;
  return `path(evenodd,"${circle(outer)}${circle(inner)}")`;
}

/** The diameter of the ring's hole, px. */
export const ringHoleSize = (size: number, hole = RING_HOLE) => Math.round(size * hole);
