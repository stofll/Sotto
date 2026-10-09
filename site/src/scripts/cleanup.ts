import type { Behaviour, Runtime } from './runtime';

/** The steps after recognition, in the order the pipeline lists them. */
const KINDS = ['tic', 'term', 'rule', 'finish'];
const WORD_MS = 140;
const STEP_MS = 850;
const HOLD_MS = 3500;
const PIECE_STATES = ['is-in', 'is-struck', 'is-gone', 'is-fixed', 'is-lit'];

/**
 * Cleanup, close up: the raw words arrive while the voice line moves, then each
 * step changes its own pieces and the pipeline marks where it is. It loops while
 * the card is on screen, in a visible tab and with motion allowed; otherwise the
 * cleaned sentence stands, as it does without JavaScript.
 */
export const initCleanup = ({ query, all, signal, observe, later, clear, reduced }: Runtime): Behaviour => {
  const card = query('[data-closeup]');
  const wave = query<HTMLCanvasElement>('[data-closeup] canvas');
  if (!card) return {};
  const pieces = all('[data-closeup] .closeup-piece');
  const steps = all('[data-closeup-step]');

  let timers: number[] = [];
  let visible = false;
  let hidden = document.hidden;
  let playing = false;

  const at = (delay: number, callback: () => void) => timers.push(later(callback, delay));
  const mark = (current: number) => steps.forEach((step, index) => {
    step.classList.toggle('is-current', index === current);
    step.classList.toggle('is-done', index < current);
  });
  const speaking = (on: boolean) => { if (wave) wave.dataset.speaking = String(on); };

  const final = () => {
    timers.forEach(clear);
    timers = [];
    playing = false;
    card.classList.remove('is-playing');
    card.classList.add('is-final');
    pieces.forEach((piece) => {
      piece.textContent = piece.dataset.to ?? '';
      piece.classList.remove(...PIECE_STATES);
    });
    mark(steps.length);
    speaking(false);
  };

  const play = () => {
    timers.forEach(clear);
    timers = [];
    playing = true;
    card.classList.remove('is-final');
    card.classList.add('is-playing');
    pieces.forEach((piece) => {
      piece.textContent = piece.dataset.from ?? '';
      piece.classList.remove(...PIECE_STATES);
    });
    mark(0);
    speaking(true);

    let time = 300;
    pieces.forEach((piece) => {
      if (!piece.dataset.from) return;
      at(time, () => piece.classList.add('is-in'));
      time += WORD_MS + (piece.dataset.from?.length ?? 0) * 10;
    });
    at(time + 150, () => speaking(false));
    time += 500;
    KINDS.forEach((kind, index) => {
      at(time, () => {
        mark(index + 1);
        pieces.filter((piece) => piece.dataset.kind === kind).forEach((piece) => {
          if (!piece.dataset.to) {
            piece.classList.add('is-struck');
            at(300, () => piece.classList.add('is-gone'));
          } else {
            piece.textContent = piece.dataset.to;
            piece.classList.add('is-in', 'is-fixed', 'is-lit');
            at(600, () => piece.classList.remove('is-lit'));
          }
        });
      });
      time += STEP_MS;
    });
    at(time, () => {
      mark(steps.length - 1);
      card.classList.add('is-final');
      // Removed pieces are collapsed by now; emptying them leaves exactly the pasted text.
      pieces.forEach((piece) => { if (piece.classList.contains('is-gone')) piece.textContent = ''; });
    });
    at(time + 400, () => mark(steps.length));
    at(time + HOLD_MS, play);
  };

  const update = () => {
    const live = visible && !hidden && !reduced();
    if (live && !playing) play();
    else if (!live && playing) final();
  };

  query('[data-closeup-replay]')?.addEventListener('click', () => (reduced() ? final() : play()), { signal });
  observe(new IntersectionObserver(([entry]) => { visible = Boolean(entry?.isIntersecting); update(); }, { threshold: 0.4 })).observe(card);

  return {
    onVisibilityChange: (value) => { hidden = value; update(); },
    onMotionChange: update,
    destroy: final,
  };
};
