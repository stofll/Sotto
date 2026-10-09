/**
 * A tab list over panels that share one grid cell: click, arrow keys, Home and
 * End. The list may stand vertically or lie flat, so both arrow pairs move.
 */
export const createTabs = (
  list: HTMLElement,
  tabs: HTMLButtonElement[],
  panels: HTMLElement[],
  signal: AbortSignal,
  onSelect: (index: number) => void = () => {},
) => {
  let current = 0;

  const select = (index: number, focus = false) => {
    current = index;
    tabs.forEach((tab, position) => {
      const selected = position === index;
      tab.setAttribute('aria-selected', String(selected));
      tab.tabIndex = selected ? 0 : -1;
    });
    panels.forEach((panel, position) => {
      const selected = position === index;
      panel.inert = !selected;
      panel.classList.toggle('is-active', selected);
      if (selected) panel.removeAttribute('aria-hidden');
      else panel.setAttribute('aria-hidden', 'true');
    });
    if (focus) tabs[index]!.focus();
    onSelect(index);
  };

  tabs.forEach((tab, index) => tab.addEventListener('click', () => select(index), { signal }));
  list.addEventListener('keydown', (event) => {
    // Arrows move from the tab that has focus, which is the selected one unless a script moved it.
    const focused = tabs.indexOf(document.activeElement as HTMLButtonElement);
    const from = focused < 0 ? current : focused;
    const moves: Record<string, number> = {
      ArrowDown: from + 1, ArrowRight: from + 1, ArrowUp: from - 1, ArrowLeft: from - 1, Home: 0, End: tabs.length - 1,
    };
    const target = moves[event.key];
    if (target === undefined) return;
    event.preventDefault();
    select((target + tabs.length) % tabs.length, true);
  }, { signal });

  /** Leaves every panel readable, as the page is without scripts. */
  const reset = () => panels.forEach((panel) => {
    panel.inert = false;
    panel.removeAttribute('aria-hidden');
    panel.classList.remove('is-active');
  });

  return { select, reset, current: () => current };
};
