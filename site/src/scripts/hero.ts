import type { Behaviour, Runtime } from './runtime';

/**
 * Hero timeline: a 9.6 s loop that reveals the sentence in semantic chunks
 * rather than character by character, so it reads as speech and not as typing.
 */
export const initHero = (runtime: Runtime): Behaviour => {
  const { query, all, signal, text, observe, strings } = runtime;

  const hero = query('[data-hero-demo]');
  const heroState = query('[data-demo-state]');
  const transcript = query('[data-demo-transcript]');
  const time = query('[data-demo-time]');
  const overlay = query('[data-overlay]');
  const pause = query<HTMLButtonElement>('[data-demo-pause]');
  const replay = query<HTMLButtonElement>('[data-demo-replay]');
  const closeOverlay = query('[data-overlay-close]');
  const steps = all<HTMLElement>('[data-step]');

  /** Index 0 is the idle prompt; 1..5 are the progressive reveal. */
  const words = ['', ...strings.demo.transcript];

  let elapsed = 0;
  let lastFrame = 0;
  let animationFrame = 0;
  let lastPaint = -100;
  let manualPaused = false;
  let heroVisible = hero ? hero.getBoundingClientRect().top < window.innerHeight : false;
  let currentPhase = '';

  const canRun = () =>
    !!hero && !runtime.reduced() && !manualPaused && heroVisible && !document.hidden && !signal.aborted;

  const updatePauseControl = () => {
    const paused = manualPaused || runtime.reduced();
    hero?.setAttribute('data-paused', String(paused || !heroVisible || document.hidden));

    if (pause) {
      pause.disabled = runtime.reduced();
      pause.setAttribute('aria-pressed', String(paused));
      pause.setAttribute(
        'aria-label',
        runtime.reduced() ? strings.demo.motionDisabled : paused ? strings.demo.resume : strings.demo.pause,
      );
    }

    const pauseIcon = query('[data-pause-icon]');
    const playIcon = query('[data-play-icon]');
    if (pauseIcon) pauseIcon.hidden = paused;
    if (playIcon) playIcon.hidden = !paused;
  };

  const paint = (forceFinal = false) => {
    if (!hero) return;
    const t = forceFinal ? 6600 : elapsed;
    const phase =
      t < 500 ? 'idle' : t < 1200 ? 'ready' : t < 4200 ? 'listening' : t < 5500 ? 'processing' : t < 9100 ? 'done' : 'reset';

    if (currentPhase !== phase) {
      currentPhase = phase;
      hero.dataset.phase = phase;
      text(heroState, strings.demo.phases[phase] ?? '');
      const activeStep = phase === 'listening' ? '2' : phase === 'processing' || phase === 'done' ? '3' : '1';
      steps.forEach((step) => step.classList.toggle('is-current', step.dataset.step === activeStep));
    }

    const chunk = t < 1400 ? 0 : t < 2000 ? 1 : t < 2450 ? 2 : t < 3050 ? 3 : t < 3650 ? 4 : 5;
    text(transcript, chunk === 0 ? strings.demo.prompt : words[chunk]);
    text(time, t < 1200 ? '0:00' : `0:0${Math.min(4, Math.floor((t - 1200) / 800))}`);
  };

  const frame = (now: number) => {
    animationFrame = 0;
    if (!canRun()) {
      lastFrame = 0;
      return;
    }
    if (lastFrame) elapsed = (elapsed + Math.min(now - lastFrame, 120)) % 9600;
    lastFrame = now;
    if (now - lastPaint >= 70) {
      paint();
      lastPaint = now;
    }
    animationFrame = requestAnimationFrame(frame);
  };

  const sync = () => {
    updatePauseControl();
    if (canRun()) {
      if (!animationFrame) {
        lastFrame = 0;
        animationFrame = requestAnimationFrame(frame);
      }
    } else {
      cancelAnimationFrame(animationFrame);
      animationFrame = 0;
      lastFrame = 0;
    }
  };

  paint(runtime.reduced());

  pause?.addEventListener(
    'click',
    () => {
      manualPaused = !manualPaused;
      sync();
    },
    { signal },
  );

  replay?.addEventListener(
    'click',
    () => {
      if (overlay) overlay.hidden = false;
      manualPaused = false;
      elapsed = 0;
      lastFrame = 0;
      lastPaint = -100;
      paint(runtime.reduced());
      sync();
    },
    { signal },
  );

  closeOverlay?.addEventListener(
    'click',
    () => {
      if (overlay) overlay.hidden = true;
      manualPaused = true;
      sync();
      replay?.focus();
    },
    { signal },
  );

  if (hero && 'IntersectionObserver' in window) {
    const heroObserver = observe(
      new IntersectionObserver(
        (entries) => {
          heroVisible = entries.some((entry) => entry.isIntersecting);
          hero.classList.toggle('is-offscreen', !heroVisible);
          sync();
        },
        { threshold: 0.1 },
      ),
    );
    heroObserver.observe(hero);
  }

  sync();

  return {
    onVisibilityChange: () => sync(),
    onMotionChange: (isReduced) => {
      if (isReduced) paint(true);
      sync();
    },
    destroy: () => {
      cancelAnimationFrame(animationFrame);
      hero?.classList.remove('is-offscreen');
    },
  };
};
