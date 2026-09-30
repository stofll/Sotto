import type { Behaviour, Runtime } from './runtime';

type WaveKind = 'stage' | 'ambient';

const ACCENT = '255,122,47';
/** Milliseconds per typed character, the pause after a phrase, and before the next. */
const CHAR_MS = 45;
const HOLD_MS = 2400;
const FIRST_DELAY_MS = 400;
const NEXT_DELAY_MS = 700;

/**
 * The voice demo: a line that swells while a phrase is "spoken" and the words
 * typing out beside it, plus the quieter line behind the closing call. One animation loop drives all of them, and it runs only while
 * one of the canvases is on screen, the tab is visible and motion is allowed.
 * Otherwise visible canvases hold a single still frame and the first phrase
 * stays written out, which is also what the page shows without JavaScript.
 */
export const initVoice = ({ all, signal, observe, reduced, strings }: Runtime): Behaviour => {
  const canvases = all<HTMLCanvasElement>('canvas[data-voice-wave]');
  const texts = all('[data-voice-text]');
  const phrases = strings.phrases;
  const [firstPhrase] = phrases;
  if (!canvases.length || firstPhrase === undefined) return {};

  const visible = new Set<HTMLCanvasElement>();
  let hidden = document.hidden;
  let frame = 0;
  let last = 0;
  let envelope = 0;
  const typing = { index: 0, shown: 0, hold: 0, wait: -FIRST_DELAY_MS };

  const fit = (canvas: HTMLCanvasElement) => {
    const ratio = window.devicePixelRatio || 1;
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    const pixelWidth = Math.round(width * ratio);
    const pixelHeight = Math.round(height * ratio);
    if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
      canvas.width = pixelWidth;
      canvas.height = pixelHeight;
    }
    const context = canvas.getContext('2d');
    context?.setTransform(ratio, 0, 0, ratio, 0, 0);
    context?.clearRect(0, 0, width, height);
    return { context, width, height };
  };

  /** Seven detuned sine strands; `symmetric` fades both ends instead of trailing off to the right. */
  const wave = (canvas: HTMLCanvasElement, energy: number, time: number, split: number, symmetric: boolean) => {
    const { context, width, height } = fit(canvas);
    if (!context || !width) return;
    const mid = height / 2;
    const amplitude = height * 0.4 * (0.1 + 0.9 * energy);
    const offset = (x: number, strand: number) => {
      const u = x / split;
      const taper = symmetric ? Math.sin(Math.PI * u) : Math.pow(1 - u, 1.3) * Math.min(1, u * 8 + 0.2);
      return taper * amplitude * (
        0.6 * Math.sin(x * 0.012 + time * 2.1 + strand * 1.1) +
        0.4 * Math.sin(x * 0.031 - time * 3.4 + strand * 0.6)
      );
    };

    const gradient = context.createLinearGradient(0, 0, split, 0);
    if (symmetric) {
      gradient.addColorStop(0, `rgba(${ACCENT},0)`);
      gradient.addColorStop(0.5, `rgba(${ACCENT},.8)`);
      gradient.addColorStop(1, `rgba(${ACCENT},0)`);
    } else {
      gradient.addColorStop(0, `rgba(${ACCENT},0)`);
      gradient.addColorStop(1, `rgba(${ACCENT},1)`);
    }
    context.strokeStyle = gradient;

    for (let strand = 0; strand < 7; strand += 1) {
      context.lineWidth = strand ? 1 : 2;
      context.beginPath();
      for (let x = 0; x <= split; x += 3) {
        const y = mid + offset(x, strand);
        if (x) context.lineTo(x, y);
        else context.moveTo(x, y);
      }
      context.stroke();
    }

    if (!symmetric) {
      // The point where the voice becomes text.
      context.fillStyle = `rgba(${ACCENT},.22)`;
      context.beginPath();
      context.arc(split, mid, 9 + 6 * energy, 0, Math.PI * 2);
      context.fill();
      context.fillStyle = `rgb(${ACCENT})`;
      context.beginPath();
      context.arc(split, mid, 4, 0, Math.PI * 2);
      context.fill();
    }
  };

  const drawCanvas = (canvas: HTMLCanvasElement, time: number, energy: number) => {
    const kind = canvas.dataset.voiceWave as WaveKind;
    if (kind === 'stage') wave(canvas, energy, time, canvas.clientWidth * 0.5, false);
    else wave(canvas, 0.35 + 0.25 * Math.sin(time * 0.7), time * 0.6, canvas.clientWidth, true);
  };

  const drawVisible = (time: number, energy: number) => {
    visible.forEach((canvas) => drawCanvas(canvas, time, energy));
  };

  const setText = (value: string) => {
    texts.forEach((element) => {
      if (element.textContent !== value) element.textContent = value;
    });
  };

  const tick = (now: number) => {
    const elapsed = Math.min(50, now - (last || now));
    last = now;

    const phrase = phrases[typing.index % phrases.length] ?? firstPhrase;
    let speaking = false;
    if (typing.shown < phrase.length) {
      typing.wait += elapsed;
      if (typing.wait > 0) {
        speaking = true;
        while (typing.wait > CHAR_MS && typing.shown < phrase.length) {
          typing.wait -= CHAR_MS;
          typing.shown += 1;
        }
      }
    } else {
      typing.hold += elapsed;
      if (typing.hold > HOLD_MS) {
        typing.index += 1;
        typing.shown = 0;
        typing.hold = 0;
        typing.wait = -NEXT_DELAY_MS;
      }
    }

    envelope += ((speaking ? 1 : 0.05) - envelope) * 0.07;
    setText(phrase.slice(0, typing.shown));
    drawVisible(now / 1000, envelope);
    frame = requestAnimationFrame(tick);
  };

  const still = () => {
    setText(firstPhrase);
    drawVisible(1.2, 0.6);
  };

  const update = () => {
    const shouldRun = visible.size > 0 && !hidden && !reduced();
    if (shouldRun && !frame) {
      last = 0;
      frame = requestAnimationFrame(tick);
    } else if (!shouldRun && frame) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
  };

  const visibility = observe(
    new IntersectionObserver((entries) => {
      entries.forEach((entry) => {
        const canvas = entry.target as HTMLCanvasElement;
        if (entry.isIntersecting) {
          if (!visible.has(canvas)) {
            visible.add(canvas);
            drawCanvas(canvas, 1.2, reduced() ? 0.6 : envelope);
          }
        } else visible.delete(canvas);
      });
      update();
    }),
  );
  canvases.forEach((canvas) => visibility.observe(canvas));

  // Canvases are sized in device pixels, so a resize would stretch a still frame.
  window.addEventListener('resize', () => { if (!frame) drawVisible(1.2, reduced() ? 0.6 : envelope); }, { passive: true, signal });

  if (reduced()) still();
  else setText('');

  return {
    onVisibilityChange: (isHidden) => {
      hidden = isHidden;
      update();
    },
    onMotionChange: () => {
      update();
      if (reduced()) still();
    },
    destroy: () => {
      cancelAnimationFrame(frame);
      frame = 0;
      setText(firstPhrase);
    },
  };
};
