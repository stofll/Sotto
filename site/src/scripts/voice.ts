import type { Behaviour, Runtime } from './runtime';

/** Where the still frame freezes the line. */
const STILL_TIME = 1.2;

/**
 * Voice lines: the quiet one behind the closing call (`ambient`) and the one
 * in the cleanup card (`speech`), which swells while its `data-speaking` is
 * true. They move only while a canvas is on screen, the tab is visible and
 * motion is allowed; otherwise visible canvases hold a single still frame.
 */
export const initVoice = ({ all, signal, observe, reduced }: Runtime): Behaviour => {
  const canvases = all<HTMLCanvasElement>('canvas[data-voice-wave]');
  if (!canvases.length) return {};
  /** Each speech line eases towards its target level rather than jumping. */
  const energies = new Map<HTMLCanvasElement, number>();

  const visible = new Set<HTMLCanvasElement>();
  let hidden = document.hidden;
  let frame = 0;

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

  /** Seven detuned sine strands, faded at both ends. */
  const wave = (canvas: HTMLCanvasElement, time: number) => {
    const { context, width, height } = fit(canvas);
    if (!context || !width) return;
    const mid = height / 2;
    let energy = 0.35 + 0.25 * Math.sin(time * 0.7);
    if (canvas.dataset.voiceWave === 'speech') {
      const target = canvas.dataset.speaking === 'true' ? 1 : 0.06;
      const previous = energies.get(canvas) ?? target;
      energy = previous + (target - previous) * 0.08;
      energies.set(canvas, energy);
    }
    const amplitude = height * 0.4 * (0.1 + 0.9 * energy);
    const phase = time * 0.6;
    const offset = (x: number, strand: number) =>
      Math.sin((Math.PI * x) / width) * amplitude * (
        0.6 * Math.sin(x * 0.012 + phase * 2.1 + strand * 1.1) +
        0.4 * Math.sin(x * 0.031 - phase * 3.4 + strand * 0.6)
      );

    // The canvas's own resolved `color`: a token declared with light-dark()
    // reads back as that expression, which a canvas cannot parse.
    context.strokeStyle = getComputedStyle(canvas).color;

    for (let strand = 0; strand < 7; strand += 1) {
      // The main strand leads; the others trail behind it, fainter.
      context.globalAlpha = strand ? 0.3 : 0.9;
      context.lineWidth = strand ? 1 : 2;
      context.beginPath();
      for (let x = 0; x <= width; x += 3) {
        const y = mid + offset(x, strand);
        if (x) context.lineTo(x, y);
        else context.moveTo(x, y);
      }
      context.stroke();
    }
    context.globalAlpha = 1;
  };

  const drawVisible = (time: number) => visible.forEach((canvas) => wave(canvas, time));

  const tick = (now: number) => {
    drawVisible(now / 1000);
    frame = requestAnimationFrame(tick);
  };

  const update = () => {
    const shouldRun = visible.size > 0 && !hidden && !reduced();
    if (shouldRun && !frame) frame = requestAnimationFrame(tick);
    else if (!shouldRun && frame) {
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
            wave(canvas, STILL_TIME);
          }
        } else visible.delete(canvas);
      });
      update();
    }),
  );
  canvases.forEach((canvas) => visibility.observe(canvas));

  // Canvases are sized in device pixels, so a resize would stretch a still frame.
  window.addEventListener('resize', () => { if (!frame) drawVisible(STILL_TIME); }, { passive: true, signal });

  return {
    onVisibilityChange: (isHidden) => {
      hidden = isHidden;
      update();
    },
    onMotionChange: () => {
      update();
      if (reduced()) drawVisible(STILL_TIME);
    },
    destroy: () => {
      cancelAnimationFrame(frame);
      frame = 0;
    },
  };
};
