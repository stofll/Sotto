import type { Behaviour, Runtime } from './runtime';

/** The app band drifts only while it is on screen, in a visible tab and with motion allowed. */
export const initApps = ({ query, observe, reduced }: Runtime): Behaviour => {
  const band = query('[data-apps]');
  if (!band) return {};

  let visible = false;
  let hidden = document.hidden;
  const update = () => band.classList.toggle('is-playing', visible && !hidden && !reduced());

  observe(new IntersectionObserver(([entry]) => { visible = Boolean(entry?.isIntersecting); update(); })).observe(band);

  return {
    onVisibilityChange: (value) => { hidden = value; update(); },
    onMotionChange: update,
    destroy: () => band.classList.remove('is-playing'),
  };
};
