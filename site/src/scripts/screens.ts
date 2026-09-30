import { tourSteps } from '../data/screen-tour';
import type { Behaviour, Runtime } from './runtime';

/** Feature walkthrough; screenshots are decoded before their crossfade begins. */
export const initScreens = ({ query, all, signal, observe, strings }: Runtime): Behaviour => {
  const section = query('[data-screens]');
  const panels = all<HTMLElement>('[data-screen-panel]');
  const tabs = all<HTMLButtonElement>('[data-screen-tabs] [role="tab"]');
  const toggle = query<HTMLButtonElement>('[data-tour-toggle]');
  const guide = query('[data-tour-guide]');
  const previous = query<HTMLButtonElement>('[data-tour-previous]');
  const next = query<HTMLButtonElement>('[data-tour-next]');
  const zoom = query<HTMLAnchorElement>('[data-screen-zoom]');
  const dialog = query<HTMLDialogElement>('[data-screen-dialog]');
  const full = query<HTMLImageElement>('[data-screen-full]');
  const themeButtons = all<HTMLButtonElement>('[data-theme-target]');
  const stepButtons = all<HTMLButtonElement>('[data-step-target]');
  const descriptions = all<HTMLElement>('[data-step-description]');
  const live = query('[data-tour-descriptions]');
  const error = query('[data-screen-error]');
  if (!section || !panels.length) return {};

  let step = 0;
  let theme = 'dark';
  let touring = false;
  let visible = false;
  let hidden = document.hidden;
  let timer = 0;
  let request = 0;
  let opener: HTMLElement | null = null;
  const panelFor = (index: number) => panels.find(panel => panel.dataset.screenPanel === tourSteps[index]!.screen)!;
  const imageFor = (index: number, appearance = theme) => panelFor(index).querySelector<HTMLImageElement>(`[data-theme-image="${appearance}"]`)!;

  const schedule = () => {
    window.clearTimeout(timer);
    guide?.classList.remove('is-playing');
    if (!touring || !visible || hidden) return;
    // Restart the single CSS progress animation together with the step timer.
    if (guide) { void guide.offsetWidth; guide.classList.add('is-playing'); }
    timer = window.setTimeout(() => {
      if (step === tourSteps.length - 1) setTouring(false);
      else void show(step + 1);
    }, 6500);
  };
  const setTouring = (value: boolean) => {
    touring = value;
    // An autoplaying tour would otherwise interrupt a screen reader every step.
    live?.setAttribute('aria-live', touring ? 'off' : 'polite');
    if (toggle) {
      toggle.textContent = touring ? strings.tour.pause : strings.tour.play;
      toggle.setAttribute('aria-pressed', String(touring));
    }
    schedule();
  };
  const render = () => {
    const current = tourSteps[step]!;
    section.dataset.theme = theme;
    panels.forEach(panel => {
      const selected = panel.dataset.screenPanel === current.screen;
      panel.inert = !selected;
      panel.setAttribute('aria-hidden', String(!selected));
      panel.querySelectorAll<HTMLElement>('[data-theme-image]').forEach(img => img.setAttribute('aria-hidden', String(img.dataset.themeImage !== theme)));
    });
    tabs.forEach(tab => {
      const selected = tab.dataset.screenTarget === current.screen;
      tab.setAttribute('aria-selected', String(selected));
      tab.tabIndex = selected ? 0 : -1;
    });
    themeButtons.forEach(button => button.setAttribute('aria-pressed', String(button.dataset.themeTarget === theme)));
    stepButtons.forEach(button => button.setAttribute('aria-pressed', String(button.dataset.stepTarget === current.id)));
    descriptions.forEach(description => { description.hidden = description.dataset.stepDescription !== current.id; });
    if (previous) previous.disabled = step === 0;
    if (next) next.disabled = step === tourSteps.length - 1;
    if (zoom) zoom.href = imageFor(step).dataset.fullSrc!;
    schedule();
  };
  const show = async (index: number, appearance = theme) => {
    const pending = ++request;
    const img = imageFor(index, appearance);
    img.loading = 'eager';
    if (img.dataset.src) {
      img.srcset = img.dataset.srcset!;
      img.src = img.dataset.src;
      delete img.dataset.src;
      delete img.dataset.srcset;
    }
    else if (img.complete && img.naturalWidth === 0) img.src = img.src;
    try { await img.decode(); }
    catch {
      if (pending === request && !signal.aborted) { if (error) error.hidden = false; setTouring(false); }
      return;
    }
    if (pending !== request || signal.aborted) return;
    if (error) error.hidden = true;
    step = index;
    theme = appearance;
    render();
  };
  const choose = (index: number) => { setTouring(false); void show(index); };
  tabs.forEach(tab => tab.addEventListener('click', () => choose(tourSteps.findIndex(item => item.screen === tab.dataset.screenTarget)), { signal }));
  query('[data-screen-tabs]')?.addEventListener('keydown', event => {
    const screen = tabs.findIndex(tab => tab.getAttribute('aria-selected') === 'true');
    const moves: Record<string, number> = { ArrowRight: (screen + 1) % tabs.length, ArrowLeft: (screen - 1 + tabs.length) % tabs.length, Home: 0, End: tabs.length - 1 };
    const index = moves[event.key];
    if (index === undefined) return;
    event.preventDefault();
    tabs[index]!.focus();
    choose(tourSteps.findIndex(item => item.screen === tabs[index]!.dataset.screenTarget));
  }, { signal });
  stepButtons.forEach(button => button.addEventListener('click', () => choose(tourSteps.findIndex(item => item.id === button.dataset.stepTarget)), { signal }));
  previous?.addEventListener('click', () => choose(Math.max(0, step - 1)), { signal });
  next?.addEventListener('click', () => choose(Math.min(tourSteps.length - 1, step + 1)), { signal });
  themeButtons.forEach(button => button.addEventListener('click', () => { setTouring(false); void show(step, button.dataset.themeTarget); }, { signal }));
  toggle?.addEventListener('click', () => {
    if (!touring && step === tourSteps.length - 1) void show(0);
    setTouring(!touring);
  }, { signal });
  // Never move a keyboard user's focus into an inactive screenshot.
  section.addEventListener('focusin', () => setTouring(false), { signal });
  zoom?.addEventListener('click', event => {
    if (!dialog || !full || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    setTouring(false);
    opener = zoom;
    full.src = zoom.href;
    full.alt = imageFor(step).alt;
    const title = query('#screen-dialog-title');
    if (title) title.textContent = tabs.find(tab => tab.getAttribute('aria-selected') === 'true')?.textContent ?? '';
    dialog.showModal();
    document.body.classList.add('dialog-open');
    const body = dialog.querySelector('.screen-dialog-body');
    if (body) { body.scrollTop = 0; body.scrollLeft = 0; }
  }, { signal });
  query('[data-screen-close]')?.addEventListener('click', () => dialog?.close(), { signal });
  dialog?.addEventListener('click', event => {
    const rect = dialog.getBoundingClientRect();
    if (event.target === dialog && (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom)) dialog.close();
  }, { signal });
  dialog?.addEventListener('close', () => { document.body.classList.remove('dialog-open'); opener?.focus(); }, { signal });
  observe(new IntersectionObserver(([entry]) => { visible = Boolean(entry?.isIntersecting); schedule(); }, { threshold: 0.25 })).observe(section);
  section.classList.add('is-enhanced');
  render();
  return {
    onVisibilityChange: value => { hidden = value; schedule(); },
    onMotionChange: reduced => { if (reduced) setTouring(false); },
    destroy: () => {
      ++request;
      window.clearTimeout(timer);
      guide?.classList.remove('is-playing');
      if (dialog?.open) dialog.close();
      section.classList.remove('is-enhanced');
      live?.setAttribute('aria-live', 'polite');
      panels.forEach(panel => { panel.inert = false; panel.removeAttribute('aria-hidden'); });
    },
  };
};
