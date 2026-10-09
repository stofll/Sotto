import type { Behaviour, Runtime } from './runtime';

/** Where the reader's own choice is kept, in their browser only. */
const KEY = 'sotto-theme';

/**
 * Runs in <head> before the first paint, so a stored choice never flashes the
 * other theme. Inline scripts are allowed by their hash alone: public/_headers
 * lists this exact text's SHA-256, and a test keeps the two in step.
 */
export const THEME_HEAD_SCRIPT = `try{var t=localStorage.getItem("${KEY}");if(t==="light"||t==="dark")document.documentElement.dataset.theme=t}catch(e){}`;

/**
 * The theme follows the system until the reader picks the other one with the
 * switch in the header. A pick that matches the system is forgotten, so the
 * page goes back to following it.
 */
export const initTheme = ({ query, all, signal }: Runtime): Behaviour => {
  const button = query<HTMLButtonElement>('[data-theme-toggle]');
  if (!button) return {};
  const root = document.documentElement;
  const system = matchMedia('(prefers-color-scheme: dark)');
  // The browser's own bar takes the page colour; each tag carries its theme's.
  const bars = all<HTMLMetaElement>('meta[name="theme-color"]');
  const barColour = Object.fromEntries(bars.map((bar) => [bar.dataset.theme, bar.content]));

  const dark = () => (root.dataset.theme ? root.dataset.theme === 'dark' : system.matches);
  const sync = () => {
    button.setAttribute('aria-pressed', String(dark()));
    const picked = root.dataset.theme;
    bars.forEach((bar) => { bar.content = barColour[picked ?? bar.dataset.theme ?? ''] ?? bar.content; });
  };
  const store = (theme: string | null) => {
    try {
      if (theme) localStorage.setItem(KEY, theme);
      else localStorage.removeItem(KEY);
    } catch {
      // Storage may be off; the choice then lasts until the page is left.
    }
  };

  button.addEventListener('click', () => {
    const next = dark() ? 'light' : 'dark';
    if ((next === 'dark') === system.matches) {
      delete root.dataset.theme;
      store(null);
    } else {
      root.dataset.theme = next;
      store(next);
    }
    sync();
  }, { signal });
  system.addEventListener('change', sync, { signal });
  sync();
  return {};
};
