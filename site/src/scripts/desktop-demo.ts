import { drawSpeech, drawStillLevel } from './voice-level';
import type { Behaviour, Runtime } from './runtime';
import type { Look, OverlayFrame, Palette, mountOverlay } from './app-overlay/island';

type DeskPhase = 'idle' | 'rec' | 'stream' | 'proc' | 'done' | 'rest';

const WORD_MS = 210;
/** How long each phase lasts, in milliseconds; streaming lasts as long as its words. */
const DURATION: Record<DeskPhase, number> = { idle: 1000, rec: 700, stream: 0, proc: 900, done: 1800, rest: 2600 };
const ORDER: DeskPhase[] = ['idle', 'rec', 'stream', 'proc', 'done', 'rest'];
/** The step caption each phase belongs to. */
const STEP: Record<DeskPhase, number> = { idle: 1, rec: 2, stream: 2, proc: 3, done: 3, rest: 3 };
const PRESS_MS = 220;
/** The level meter does not need more than 30 frames a second. */
const FRAME_MS = 33;
/** What the app's overlay shows in each phase of the desk. */
const SCENE: Record<DeskPhase, Pick<OverlayFrame, 'phase' | 'shown'>> = {
  idle: { phase: 'recording', shown: false },
  rec: { phase: 'recording', shown: true },
  stream: { phase: 'recording', shown: true },
  proc: { phase: 'processing', shown: true },
  done: { phase: 'pasted', shown: true },
  rest: { phase: 'pasted', shown: false },
};

/**
 * The hero's desktop: the hotkey starts a recording, the overlay streams the
 * raw words, a second press stops it and the cleaned text lands in the field.
 * It plays only while on screen, in a visible tab and with motion allowed;
 * otherwise it holds the moment after the paste, which is also the picture
 * without JavaScript.
 *
 * Once the desk is in view, the app's own overlay loads in the background and
 * replaces the stand-in pill, together with the choice of look, text timing
 * and palette beneath the desk.
 */
export const initDesktopDemo = ({ query, all, observe, later, text, reduced, signal }: Runtime): Behaviour => {
  const desk = query('[data-desk]');
  const overlay = query<HTMLElement>('[data-desk] [data-ov]');
  const field = query('[data-desk] [data-desk-text]');
  if (!desk || !overlay || !field) return {};
  const bars = all('[data-desk] [data-ov-bar]');
  const timer = query('[data-desk] [data-ov-timer]');
  const draft = query('[data-desk] [data-ov-draft]');
  const tail = query('[data-desk] [data-ov-tail]');
  const pasted = field.textContent ?? '';
  const words = `${draft?.textContent ?? ''} ${tail?.textContent ?? ''}`.trim().split(' ');
  const host = query('[data-desk] [data-desk-island]');
  const tune = query('[data-desk-tune]');
  const lookButtons = all<HTMLButtonElement>('[data-desk-tune] [data-look]');
  const order = lookButtons.map((button) => button.dataset.look).filter(Boolean) as Look[];
  const processing = query('[data-desk] .ov-proc .ov-lbl')?.textContent ?? '';
  const inserted = query('[data-desk] .ov-done .ov-lbl')?.textContent ?? '';

  let island: ReturnType<typeof mountOverlay> | null = null;
  let loading = false;
  let picked: Look | null = null;
  let streaming = true;
  let palette: Palette = 'graphite';
  /** Dictations so far: until a look is picked, each takes the next one. */
  let take = 0;
  let spoken = words.length;
  let seconds = 0;

  let phase: DeskPhase = 'done';
  let phaseStart = 0;
  let frame = 0;
  let lastDraw = 0;
  let visible = false;
  let hidden = document.hidden;

  /** The first `count` words, the last two softer: the part a streaming model may still change. */
  const showWords = (count: number) => {
    spoken = count;
    const cut = Math.max(0, count - 2);
    text(draft, words.slice(0, cut).join(' '));
    text(tail, words.slice(cut, count).join(' '));
  };

  const look = () => picked ?? order[Math.max(0, take - 1) % order.length]!;

  const drawIsland = () => {
    if (!island) return;
    // Off-screen the stand-in pill holds the picture: the app's Glow keeps a frame loop while mounted.
    const shown = visible && !hidden;
    desk.classList.toggle('has-island', shown);
    if (!shown) {
      island.clear();
      return;
    }
    island.draw({
      ...SCENE[phase],
      draft: words.slice(0, spoken).join(' '),
      timer: `00:${String(seconds).padStart(2, '0')}`,
      status: phase === 'proc' ? processing : inserted,
      streaming,
      look: look(),
      palette,
      take,
      live: frame !== 0,
    });
    lookButtons.forEach((button) => button.toggleAttribute('data-live', !picked && button.dataset.look === look()));
  };

  const duration = (id: DeskPhase) => (id === 'stream' ? words.length * WORD_MS + 400 : DURATION[id]);

  /** A press of the hotkey, shown on the keycaps. */
  const press = () => {
    desk.classList.add('keys-down');
    later(() => desk.classList.remove('keys-down'), PRESS_MS);
  };

  const setPhase = (next: DeskPhase, now: number) => {
    phase = next;
    phaseStart = now;
    desk.dataset.phase = next;
    desk.dataset.step = String(STEP[next]);
    // The overlay has no idle or rest state; it keeps its last one while hidden.
    if (next !== 'idle' && next !== 'rest') overlay.dataset.phase = next;
    overlay.dataset.draft = next === 'stream' ? '1' : '0';
    if (next === 'idle') {
      take += 1;
      seconds = 0;
      text(field, '');
      showWords(0);
      text(timer, '0:00');
    }
    if (next === 'rec' || next === 'proc') press();
    if (next === 'done') text(field, pasted);
    drawIsland();
  };

  const still = () => {
    phase = 'done';
    desk.dataset.phase = 'done';
    desk.dataset.step = '3';
    overlay.dataset.phase = 'done';
    overlay.dataset.draft = '0';
    text(field, pasted);
    showWords(words.length);
    drawStillLevel(bars);
    drawIsland();
  };

  const tick = (now: number) => {
    frame = requestAnimationFrame(tick);
    if (now - lastDraw < FRAME_MS) return;
    lastDraw = now;

    const elapsed = now - phaseStart;
    if (elapsed > duration(phase)) {
      setPhase(ORDER[(ORDER.indexOf(phase) + 1) % ORDER.length]!, now);
      return;
    }
    if (phase !== 'rec' && phase !== 'stream') return;
    // The app's overlay draws its own level; the stand-in pill needs this one.
    if (!island) overlay.style.setProperty('--ov-energy', drawSpeech(bars, now / 1000).toFixed(2));
    const recorded = Math.floor((phase === 'stream' ? elapsed + duration('rec') : elapsed) / 1000);
    const count = phase === 'stream' ? Math.min(words.length, Math.max(1, Math.ceil(elapsed / WORD_MS))) : 0;
    if (recorded === seconds && count === spoken) return;
    seconds = recorded;
    text(timer, `0:${String(recorded).padStart(2, '0')}`);
    showWords(count);
    drawIsland();
  };

  /** Loaded while the browser is idle; if it fails, the stand-in pill stays. */
  const loadIsland = () => {
    if (loading || !host || !tune) return;
    loading = true;
    const start = () => import('./app-overlay/island').then(({ mountOverlay }) => {
      island = mountOverlay(host);
      tune.hidden = false;
      drawIsland();
    }, (error: unknown) => console.warn('The app overlay failed to load:', error));
    if ('requestIdleCallback' in window) requestIdleCallback(start, { timeout: 2000 });
    else later(start, 200);
  };

  /** A new choice starts a new dictation, or redraws the still picture. */
  const restart = () => (frame ? setPhase('idle', performance.now()) : drawIsland());
  const choose = (button: HTMLButtonElement) =>
    button.parentElement?.querySelectorAll('button').forEach((each) => each.setAttribute('aria-pressed', String(each === button)));
  tune?.addEventListener('click', (event) => {
    const button = (event.target as Element).closest<HTMLButtonElement>('button');
    if (!button) return;
    choose(button);
    if (button.dataset.look !== undefined) {
      picked = (button.dataset.look || null) as Look | null;
      restart();
    } else if (button.dataset.streaming) {
      streaming = button.dataset.streaming === 'true';
      restart();
    } else if (button.dataset.palette) {
      palette = button.dataset.palette as Palette;
      drawIsland();
    }
  }, { signal });

  const update = () => {
    const live = visible && !hidden && !reduced();
    desk.classList.toggle('is-running', live);
    if (live && !frame) {
      // Start from an empty field rather than mid-sentence.
      setPhase('idle', performance.now());
      frame = requestAnimationFrame(tick);
    } else if (!live && frame) {
      cancelAnimationFrame(frame);
      frame = 0;
      still();
    } else if (!live) drawIsland();
  };

  observe(new IntersectionObserver(([entry]) => {
    visible = Boolean(entry?.isIntersecting);
    if (visible) loadIsland();
    update();
  }, { threshold: 0.3 })).observe(desk);
  still();

  return {
    onVisibilityChange: (value) => { hidden = value; update(); },
    onMotionChange: update,
    destroy: () => {
      cancelAnimationFrame(frame);
      frame = 0;
      desk.classList.remove('is-running', 'keys-down', 'has-island');
      island?.destroy();
      island = null;
      if (tune) tune.hidden = true;
      still();
    },
  };
};
