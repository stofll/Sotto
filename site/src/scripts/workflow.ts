import type { Behaviour, Runtime } from './runtime';

/** Ratio at which the demo runs, and the one below which it is considered gone. */
const PLAY = 0.25;
const ARM = 0.01;
/** How long the finished message stays readable before the loop starts over. */
const HOLD = 2600;

/**
 * The secondary demo loops for as long as it is on screen, the way the hero
 * timeline does, and settles on the finished message whenever it is not: off
 * screen, on a hidden tab, or with reduced motion asked for. Tabs use standard
 * arrow/Home/End semantics, and picking one restarts the loop on that scenario.
 */
export const initWorkflow = (runtime: Runtime): Behaviour => {
  const { query, all, signal, text, observe, later, clear, strings } = runtime;

  const panel = query('#workflow-panel');
  const body = query('[data-workflow-text]');
  const miniOverlay = query('[data-mini-overlay]');
  const tabs = all<HTMLElement>('[data-workflow]');

  // Scenarios come from the dictionary the page rendered. An empty one means
  // there is no demo to play, which is more honest than starting from undefined
  // and throwing on the first read of `chunks`.
  const [first] = Object.values(strings.workflow);
  if (!first) return {};
  let selected = first;
  let running: number[] = [];
  let onScreen = false;

  const stop = () => {
    running.forEach(clear);
    running = [];
    body?.classList.remove('is-typing');
    miniOverlay?.classList.remove('is-active');
  };

  /** Ends on the complete message and stays there. */
  const settle = () => {
    stop();
    text(body, selected.chunks.join(' '));
  };

  const canRun = () => onScreen && !runtime.reduced() && !document.hidden;

  const animate = () => {
    stop();
    if (!canRun()) {
      settle();
      return;
    }

    text(body, '');
    body?.classList.add('is-typing');
    miniOverlay?.classList.add('is-active');

    const { chunks } = selected;
    chunks.forEach((_, index) =>
      running.push(later(() => text(body, chunks.slice(0, index + 1).join(' ')), 160 + index * 420)),
    );
    running.push(
      later(() => {
        settle();
        running.push(later(animate, HOLD));
      }, chunks.length * 420 + 280),
    );
  };

  const choose = (id: string, moveFocus = false) => {
    const scenario = strings.workflow[id];
    if (!scenario) return;
    selected = scenario;

    tabs.forEach((tab) => {
      const isSelected = tab.dataset.workflow === id;
      tab.setAttribute('aria-selected', String(isSelected));
      tab.tabIndex = isSelected ? 0 : -1;
      tab.classList.toggle('is-selected', isSelected);
      if (isSelected && moveFocus) tab.focus();
    });

    panel?.setAttribute('aria-labelledby', `tab-${id}`);
    text(query('[data-workflow-app]'), scenario.app);
    text(query('[data-workflow-destination]'), scenario.destination);
    // charAt, not [0]: an empty string in the dictionary yields '' rather than
    // undefined, so the avatar goes blank instead of reading "undefined".
    text(query('[data-recipient-initial]'), scenario.destination.charAt(0));
    text(query('[data-workflow-context]'), scenario.context);
    animate();
  };

  tabs.forEach((tab, index) => {
    tab.addEventListener('click', () => choose(tab.dataset.workflow ?? ''), { signal });
    tab.addEventListener(
      'keydown',
      (event) => {
        let next = index;
        if (event.key === 'ArrowDown' || event.key === 'ArrowRight') next = (index + 1) % tabs.length;
        else if (event.key === 'ArrowUp' || event.key === 'ArrowLeft') next = (index - 1 + tabs.length) % tabs.length;
        else if (event.key === 'Home') next = 0;
        else if (event.key === 'End') next = tabs.length - 1;
        else return;

        event.preventDefault();
        choose(tabs[next]?.dataset.workflow ?? '', true);
      },
      { signal },
    );
  });

  if (panel && 'IntersectionObserver' in window) {
    const observer = observe(
      new IntersectionObserver(
        (entries) => {
          const entry = entries[entries.length - 1];
          if (!entry) return;
          panel.classList.toggle('is-offscreen', !entry.isIntersecting);

          // Two thresholds, so a scroll that hovers on the edge cannot restart
          // the demo on every jitter.
          if (entry.intersectionRatio >= PLAY) {
            if (!onScreen) {
              onScreen = true;
              animate();
            }
          } else if (entry.intersectionRatio <= ARM && onScreen) {
            onScreen = false;
            settle();
          }
        },
        { threshold: [0, ARM, PLAY] },
      ),
    );
    observer.observe(panel);
  }

  return {
    onVisibilityChange: (hidden) => (hidden ? settle() : animate()),
    onMotionChange: (isReduced) => (isReduced ? settle() : animate()),
    destroy: stop,
  };
};
