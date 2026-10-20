// Outlines of the shells that are not a rounded rectangle. The overlay window
// is a transparent rectangle; a shell cuts its shape out with `clip-path`,
// which the browser also uses for hit-testing, so the torn edge and the
// ring's hole do not count as the overlay for hover (see isOverlayBody).

/** The ring's hole as a share of its outer radius. */
export const RING_HOLE = 0.56;

const round = (value: number) => Math.round(value * 100) / 100;

/** A strip whose bottom edge is torn into small teeth, as a CSS `clip-path`. */
export function tornEdgeClip(width: number, height: number, tooth = 10, depth = 6): string {
  const steps = Math.max(2, Math.round(width / (tooth / 2)));
  const points = ["0 0", `${width}px 0`];
  // Walking from the right edge to the left, every other point is a tooth's root.
  for (let step = 0; step <= steps; step++) {
    const x = width - (step * width) / steps;
    points.push(`${round(x)}px ${step % 2 ? height - depth : height}px`);
  }
  return `polygon(${points.join(",")})`;
}

/** A donut: the outer circle of a square shell with the hole cut out of it, as a CSS `clip-path`. */
export function ringClip(size: number, hole = RING_HOLE): string {
  const outer = size / 2, inner = outer * hole;
  const circle = (radius: number) => `M${round(outer - radius)} ${round(outer)}a${round(radius)} ${round(radius)} 0 1 0 ${round(radius * 2)} 0a${round(radius)} ${round(radius)} 0 1 0 ${round(-radius * 2)} 0Z`;
  return `path(evenodd,"${circle(outer)}${circle(inner)}")`;
}

/** The diameter of the ring's hole, px. */
export const ringHoleSize = (size: number, hole = RING_HOLE) => Math.round(size * hole);
