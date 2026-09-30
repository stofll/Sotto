import type { Behaviour, Runtime } from './runtime';

/** The hotkey-mode switch on the first feature card. It only swaps the illustration. */
export const initModes = ({ query, all, signal }: Runtime): Behaviour => {
  const root = query('[data-modes]');
  if (!root) return {};

  const buttons = all<HTMLButtonElement>('[data-modes] [data-mode]');
  const select = (mode: string) => {
    buttons.forEach((button) => button.setAttribute('aria-pressed', String(button.dataset.mode === mode)));
    root.querySelectorAll<HTMLElement>('[data-mode-panel], [data-mode-hint]').forEach((element) => {
      element.hidden = (element.dataset.modePanel ?? element.dataset.modeHint) !== mode;
    });
  };

  buttons.forEach((button) =>
    button.addEventListener('click', () => select(button.dataset.mode ?? 'toggle'), { signal }),
  );
  return {};
};
