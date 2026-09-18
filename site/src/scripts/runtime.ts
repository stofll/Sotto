import type { Dictionary } from '../i18n/types';

/**
 * Shared plumbing for the behaviour modules: scoped queries, timers that clean
 * themselves up, and the reduced-motion signal every animation respects.
 */

export interface Runtime {
  signal: AbortSignal;
  query: <T extends Element = HTMLElement>(selector: string) => T | null;
  all: <T extends Element = HTMLElement>(selector: string) => T[];
  /** setTimeout that is cancelled on teardown. */
  later: (callback: () => void, delay: number) => number;
  clear: (id: number) => void;
  /** Writes textContent only when it actually changed. */
  text: (element: Element | null, value: string) => void;
  observe: (observer: IntersectionObserver) => IntersectionObserver;
  reduced: () => boolean;
  strings: RuntimeStrings;
}

/** A module may ask to be told about the two page-wide state changes. */
export interface Behaviour {
  onVisibilityChange?: (hidden: boolean) => void;
  onMotionChange?: (reduced: boolean) => void;
  destroy?: () => void;
}

export interface RuntimeStrings {
  demo: {
    phases: Record<string, string>;
    prompt: string;
    transcript: string[];
    pause: string;
    resume: string;
    motionDisabled: string;
  };
  models: Record<string, string>;
  workflow: Record<string, { app: string; destination: string; context: string; chunks: string[] }>;
  copy: { copied: string; copiedTitle: string; copiedAria: string; manual: string; fallbackTitle: string; aria: string };
  /** The shape comes from the dictionary. An approximate Record would hide a
   *  mismatch until runtime, where `steps.map` would throw on undefined. */
  dialog: Dictionary['dialog'];
  nav: { openMenu: string; closeMenu: string };
}

/** Reads the strings the page rendered for the active locale. */
export const readStrings = (): RuntimeStrings => {
  const element = document.getElementById('runtime-strings');
  if (!element?.textContent) throw new Error('Runtime strings are missing from the page');
  return JSON.parse(element.textContent) as RuntimeStrings;
};

export const createRuntime = (abort: AbortController) => {
  const timers = new Set<number>();
  const observers: IntersectionObserver[] = [];
  const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
  let reduced = motion.matches;

  const runtime: Runtime = {
    signal: abort.signal,
    query: (selector) => document.querySelector(selector),
    all: (selector) => Array.from(document.querySelectorAll(selector)),
    later: (callback, delay) => {
      const id = window.setTimeout(() => {
        timers.delete(id);
        if (!abort.signal.aborted) callback();
      }, delay);
      timers.add(id);
      return id;
    },
    clear: (id) => {
      clearTimeout(id);
      timers.delete(id);
    },
    text: (element, value) => {
      if (element && element.textContent !== value) element.textContent = value;
    },
    observe: (observer) => {
      observers.push(observer);
      return observer;
    },
    reduced: () => reduced,
    strings: readStrings(),
  };

  const setReduced = (value: boolean) => {
    reduced = value;
  };

  const teardown = () => {
    abort.abort();
    timers.forEach(clearTimeout);
    timers.clear();
    observers.forEach((observer) => observer.disconnect());
  };

  return { runtime, motion, setReduced, teardown };
};

/** `{name}` substitution, matching the build-time helper in src/i18n. */
export const format = (template: string, values: Record<string, string | number>): string =>
  template.replace(/\{(\w+)\}/g, (match, key: string) => String(values[key] ?? match));
