import type { Behaviour, Runtime } from './runtime';

/**
 * The secondary demo runs once on entry, then only when the reader deliberately
 * picks a different scenario. Tabs use standard arrow/Home/End semantics.
 */
export const initWorkflow = (runtime: Runtime): Behaviour => {
  const { query, all, signal, text, observe, later, clear, strings } = runtime;

  const panel = query('#workflow-panel');
  const body = query('[data-workflow-text]');
  const miniOverlay = query('[data-mini-overlay]');
  const tabs = all<HTMLElement>('[data-workflow]');

  const ids = Object.keys(strings.workflow);
  let selected = strings.workflow[ids[0]];
  let running: number[] = [];
  let started = false;

  const stop = () => {
    running.forEach(clear);
    running = [];
    body?.classList.remove('is-typing');
    miniOverlay?.classList.remove('is-active');
  };

  const finish = () => {
    stop();
    text(body, selected.chunks.join(' '));
  };

  const animate = () => {
    stop();
    if (runtime.reduced() || document.hidden) {
      finish();
      return;
    }

    text(body, '');
    body?.classList.add('is-typing');
    miniOverlay?.classList.add('is-active');

    const { chunks } = selected;
    chunks.forEach((_, index) =>
      running.push(later(() => text(body, chunks.slice(0, index + 1).join(' ')), 160 + index * 420)),
    );
    running.push(later(finish, chunks.length * 420 + 280));
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
    text(query('[data-recipient-initial]'), scenario.destination[0]);
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
        choose(tabs[next].dataset.workflow ?? '', true);
      },
      { signal },
    );
  });

  if (panel && 'IntersectionObserver' in window) {
    const observer = observe(
      new IntersectionObserver(
        (entries) => {
          const visible = entries.some((entry) => entry.isIntersecting);
          panel.classList.toggle('is-offscreen', !visible);
          if (visible && !started) {
            started = true;
            animate();
          } else if (!visible && started) {
            finish();
          }
        },
        { threshold: 0.25 },
      ),
    );
    observer.observe(panel);
  }

  return {
    onVisibilityChange: (hidden) => {
      if (hidden) finish();
    },
    onMotionChange: (isReduced) => {
      if (isReduced) finish();
    },
    destroy: stop,
  };
};
