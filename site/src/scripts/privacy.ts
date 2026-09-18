import type { Behaviour, Runtime } from './runtime';

/** Ratio at which the pulse runs, and the one below which the next run is armed. */
const PLAY = 0.3;
const ARM = 0.01;

/**
 * The signal diagram pulses each time it comes back into view. Dropping the
 * class on the way out is what arms the replay: re-adding it on the next entry
 * restarts the animation, and the reset happens off-screen where nobody sees it.
 *
 * The two thresholds are hysteresis. With a single one, a scroll that hovered
 * around the edge would re-arm and restart the pulse on every jitter.
 */
export const initPrivacyDiagram = ({ query, observe, reduced }: Runtime): Behaviour => {
  const diagram = query('[data-privacy-diagram]');

  if (diagram && 'IntersectionObserver' in window && !reduced()) {
    const observer = observe(
      new IntersectionObserver(
        (entries) => {
          const entry = entries[entries.length - 1];
          if (!entry) return;
          diagram.classList.toggle('is-offscreen', !entry.isIntersecting);
          if (entry.intersectionRatio >= PLAY) {
            if (!reduced()) diagram.classList.add('is-playing');
          } else if (entry.intersectionRatio <= ARM) {
            diagram.classList.remove('is-playing');
          }
        },
        { threshold: [0, ARM, PLAY] },
      ),
    );
    observer.observe(diagram);
  }

  return {
    onMotionChange: (isReduced) => {
      if (isReduced) diagram?.classList.remove('is-playing');
    },
    destroy: () => diagram?.classList.remove('is-playing', 'is-offscreen'),
  };
};
