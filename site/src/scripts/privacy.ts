import type { Behaviour, Runtime } from './runtime';

/** The signal diagram pulses once, the first time it comes into view. */
export const initPrivacyDiagram = ({ query, observe, reduced }: Runtime): Behaviour => {
  const diagram = query('[data-privacy-diagram]');
  let played = false;

  if (diagram && 'IntersectionObserver' in window && !reduced()) {
    const observer = observe(
      new IntersectionObserver(
        (entries) => {
          const visible = entries.some((entry) => entry.isIntersecting);
          diagram.classList.toggle('is-offscreen', !visible);
          if (visible && !played) {
            played = true;
            diagram.classList.add('is-playing');
          }
        },
        { threshold: 0.3 },
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
