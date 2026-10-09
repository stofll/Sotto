import { drawSpeech, drawStillLevel } from './voice-level';
import type { Behaviour, Runtime } from './runtime';
import { createTabs } from './tabs';

/** How long each feature stays before the next while nobody has picked one. */
const ADVANCE_MS = 5000;
const FRAME_MS = 33;

/**
 * Features: a list on the wallpaper beside one window at a time. The list
 * advances by itself while the stage is on screen, until the reader points at,
 * focuses or picks a feature; the voice window's level moves while it is shown. Nothing moves
 * with reduced motion or in a hidden tab.
 */
export const initFeatures = ({ query, all, signal, observe, reduced }: Runtime): Behaviour => {
  const root = query('[data-caps]');
  const list = query('[data-caps] [role="tablist"]');
  const tabs = all<HTMLButtonElement>('[data-caps] [role="tab"]');
  const panels = all('[data-caps] [role="tabpanel"]');
  if (!root || !list || !tabs.length) return {};
  const bars = all('[data-cap="voice"] [data-ov-bar]');
  const voiceIndex = panels.findIndex((panel) => panel.dataset.cap === 'voice');

  let chosen = false;
  let visible = false;
  let hidden = document.hidden;
  let advance = 0;
  let frame = 0;
  let lastDraw = 0;

  const tick = (now: number) => {
    frame = requestAnimationFrame(tick);
    if (now - lastDraw < FRAME_MS) return;
    lastDraw = now;
    drawSpeech(bars, now / 1000);
  };

  const update = () => {
    const live = visible && !hidden && !reduced();
    const speaking = live && tablist.current() === voiceIndex;
    if (speaking && !frame) frame = requestAnimationFrame(tick);
    else if (!speaking && frame) {
      cancelAnimationFrame(frame);
      frame = 0;
      drawStillLevel(bars);
    }
    const advancing = live && !chosen;
    if (advancing && !advance) advance = window.setInterval(() => tablist.select((tablist.current() + 1) % tabs.length), ADVANCE_MS);
    else if (!advancing && advance) {
      clearInterval(advance);
      advance = 0;
    }
  };

  const tablist = createTabs(list, tabs, panels, signal, update);
  // Focus or a pointer on the features, or a click from assistive tech that moves neither, stops the automatic walk.
  const choose = () => { chosen = true; update(); };
  root.addEventListener('focusin', choose, { signal });
  root.addEventListener('pointerenter', choose, { signal });
  list.addEventListener('click', choose, { signal });

  observe(new IntersectionObserver(([entry]) => { visible = Boolean(entry?.isIntersecting); update(); }, { threshold: 0.3 })).observe(root);
  drawStillLevel(bars);
  root.classList.add('is-enhanced');
  tablist.select(0);

  return {
    onVisibilityChange: (value) => { hidden = value; update(); },
    onMotionChange: update,
    destroy: () => {
      cancelAnimationFrame(frame);
      clearInterval(advance);
      root.classList.remove('is-enhanced');
      tablist.reset();
    },
  };
};
