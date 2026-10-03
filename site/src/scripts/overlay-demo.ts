import { overlayPalettes, type OverlayPalette, type OverlayPhase, type OverlayTemplate } from '../data/feature-tour';
import type { Runtime } from './runtime';
import type { SceneDemo } from './tour';

/** How long each state lasts while the demo plays by itself, in milliseconds. */
const PHASES: [OverlayPhase, number][] = [['rec', 2400], ['stream', 3800], ['proc', 1400], ['done', 1800]];
/** Bars and level change at most this often; the eye does not need 60 fps from a level meter. */
const FRAME_MS = 33;

/** A speaking voice: syllable-rate swells over a slower phrase envelope. */
const voice = (time: number) => {
  const phrase = 0.55 + 0.45 * Math.sin(time * 1.3);
  const syllables = 0.5 + 0.5 * Math.sin(time * 7.3) * Math.sin(time * 3.1 + 1);
  return Math.max(0.08, Math.min(1, phrase * syllables * 1.2));
};

/**
 * The overlay scene: template, colour and state switches, and a level that
 * follows a simulated voice. Pressing a state stops the automatic cycle, so a
 * reader can study one; template and colour leave it running.
 */
export const createOverlayDemo = (scene: HTMLElement, { signal, text }: Runtime): SceneDemo => {
  const stage = scene.querySelector<HTMLElement>('[data-ov-stage]')!;
  const shell = scene.querySelector<HTMLElement>('[data-ov]')!;
  const bars = Array.from(scene.querySelectorAll<HTMLElement>('[data-ov-bar]'));
  const timer = scene.querySelector('[data-ov-timer]');
  const draft = scene.querySelector('[data-ov-draft]');
  const words = (draft?.textContent ?? '').split(' ');
  const buttons = (attribute: string) => Array.from(scene.querySelectorAll<HTMLButtonElement>(`[${attribute}]`));
  const templates = buttons('data-ov-template');
  const palettes = buttons('data-ov-palette');
  const phases = buttons('data-ov-phase');

  let phase = scene.dataset.phase as OverlayPhase;
  let auto = true;
  let frame = 0;
  let lastDraw = 0;
  let phaseStart = 0;
  let recordingStart = 0;

  const press = (group: HTMLButtonElement[], attribute: string, value: string) =>
    group.forEach((button) => button.setAttribute('aria-pressed', String(button.getAttribute(attribute) === value)));

  const setPhase = (next: OverlayPhase, now: number) => {
    phase = next;
    phaseStart = now;
    if (next === 'rec') recordingStart = now;
    scene.dataset.phase = next;
    press(phases, 'data-ov-phase', next);
  };

  const draw = (level: number, bar: (index: number) => number) => {
    shell.style.setProperty('--level', level.toFixed(3));
    bars.forEach((element, index) => { element.style.transform = `scaleY(${bar(index).toFixed(3)})`; });
  };

  const tick = (now: number) => {
    frame = requestAnimationFrame(tick);
    if (now - lastDraw < FRAME_MS) return;
    lastDraw = now;

    const elapsed = now - phaseStart;
    const duration = PHASES.find(([id]) => id === phase)![1];
    if (auto && elapsed > duration) {
      const next = PHASES[(PHASES.findIndex(([id]) => id === phase) + 1) % PHASES.length]![0];
      setPhase(next, now);
    }

    const speaking = phase === 'rec' || phase === 'stream';
    const time = now / 1000;
    if (speaking) draw(voice(time), (index) => 0.2 + 0.8 * voice(time - index * 0.06) * (0.55 + 0.45 * Math.abs(Math.sin(index * 1.9 + time * 5))));
    text(timer, `0:${String(Math.min(59, Math.floor((now - recordingStart) / 1000))).padStart(2, '0')}`);
    // The draft grows word by word while it streams; a pressed state shows it whole.
    const shown = phase === 'stream' && auto ? Math.ceil((elapsed / duration) * words.length * 1.25) : words.length;
    text(draft, words.slice(0, Math.max(1, shown)).join(' '));
  };

  /** One still frame: what the page shows without JavaScript or with reduced motion. */
  const still = () => {
    draw(0.6, (index) => 0.3 + 0.6 * Math.abs(Math.sin(index * 1.3 + 0.4)));
    text(timer, '0:12');
    text(draft, words.join(' '));
  };

  const run = (on: boolean) => {
    if (on && !frame) {
      // Restart from the beginning of a recording rather than mid-sentence.
      if (auto) setPhase('rec', performance.now());
      frame = requestAnimationFrame(tick);
    } else if (!on && frame) {
      cancelAnimationFrame(frame);
      frame = 0;
      still();
    }
  };

  templates.forEach((button) => button.addEventListener('click', () => {
    const value = button.dataset.ovTemplate as OverlayTemplate;
    scene.dataset.template = value;
    press(templates, 'data-ov-template', value);
  }, { signal }));
  palettes.forEach((button) => button.addEventListener('click', () => {
    const [hue, chroma] = overlayPalettes[button.dataset.ovPalette as OverlayPalette];
    stage.style.setProperty('--ov-hue', String(hue));
    stage.style.setProperty('--ov-chroma', String(chroma));
    press(palettes, 'data-ov-palette', button.dataset.ovPalette!);
  }, { signal }));
  phases.forEach((button) => button.addEventListener('click', () => {
    auto = false;
    setPhase(button.dataset.ovPhase as OverlayPhase, performance.now());
    if (!frame) still();
  }, { signal }));

  still();
  return { run, destroy: () => run(false) };
};
