import type { Runtime } from './runtime';
import type { SceneDemo } from './tour';

/** The panel's real states, with dwell times that keep the loop readable. */
const STATES: [string, number][] = [['idle', 1400], ['reading', 1100], ['transcribing', 2600], ['done', 5200]];

/**
 * The file scene walks through the transcription panel's states in a loop.
 * The app reports no progress, so neither does the demo: only the state text changes.
 */
export const createFileDemo = (scene: HTMLElement, { later, clear }: Runtime): SceneDemo => {
  let timer = 0;
  let running = false;

  const show = (index: number) => {
    const [state, dwell] = STATES[index]!;
    scene.dataset.state = state;
    timer = later(() => show((index + 1) % STATES.length), dwell);
  };

  const run = (on: boolean) => {
    if (on === running) return;
    running = on;
    clear(timer);
    if (on) show(0);
    else scene.dataset.state = 'done';
  };

  return { run, destroy: () => run(false) };
};
