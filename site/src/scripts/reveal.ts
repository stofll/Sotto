import type { Behaviour, Runtime } from './runtime';

/**
 * Reveals a group once it scrolls into view. Content stays visible when
 * JavaScript or IntersectionObserver is unavailable, so this is decoration only.
 */
export const initReveal = ({ all, observe, reduced }: Runtime): Behaviour => {
  const reveals = all('[data-reveal]');

  if ('IntersectionObserver' in window && !reduced()) {
    const revealObserver = observe(
      new IntersectionObserver(
        (entries) => {
          entries.forEach((entry) => {
            if (entry.isIntersecting) {
              entry.target.classList.add('is-visible');
              revealObserver.unobserve(entry.target);
            }
          });
        },
        { threshold: 0.14, rootMargin: '0px 0px -24px 0px' },
      ),
    );

    reveals.forEach((element) => {
      if (element.getBoundingClientRect().top >= window.innerHeight - 24) {
        element.classList.add('reveal-ready');
        revealObserver.observe(element);
      }
    });
  }

  return {
    onMotionChange: (isReduced) => {
      if (isReduced) reveals.forEach((element) => element.classList.add('is-visible'));
    },
    destroy: () => reveals.forEach((element) => element.classList.remove('reveal-ready', 'is-visible')),
  };
};
