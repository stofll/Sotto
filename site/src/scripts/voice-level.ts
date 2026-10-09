/** A speaking voice: syllable-rate swells over a slower phrase envelope, with
 *  short pauses between phrases, at `time` seconds. */
const voice = (time: number) => {
  const phrase = 0.55 + 0.45 * Math.sin(time * 1.3);
  const syllables = 0.5 + 0.5 * Math.sin(time * 7.3) * Math.sin(time * 3.1 + 1);
  return Math.max(0, Math.min(1, phrase * syllables * 1.25 - 0.06));
};

/** How often the app's level arrives, seconds; one bar per reading. */
const READING_S = 0.05;
/** Below this the app draws a bar unlit (`IDLE_LEVEL` in OverlayWaveform.tsx). */
const IDLE_LEVEL = 0.04;
const jitter = (n: number) => {
  const x = Math.sin(n * 12.9898) * 43758.5453;
  return x - Math.floor(x);
};
const reading = (n: number) => voice(n * READING_S) * (0.5 + 0.5 * jitter(n));

/** One bar the way OverlayWaveform.tsx paints it: height, glow and whether it is lit. */
const paint = (bar: HTMLElement, level: number) => {
  const visual = Math.sqrt(level);
  bar.style.height = `${Math.max(5, Math.round(5 + visual * 21))}px`;
  bar.style.setProperty('--ov-glow', `${(6 + visual * 8).toFixed(1)}px`);
  bar.style.setProperty('--ov-glow-opacity', (0.35 + visual * 0.35).toFixed(2));
  const on = String(level > IDLE_LEVEL);
  if (bar.dataset.on !== on) bar.dataset.on = on;
};

/**
 * The overlay's level history while someone speaks: the newest reading in the
 * last bar, older ones moving left. Returns the newest level, which the
 * overlay's inner glow follows.
 */
export const drawSpeech = (bars: HTMLElement[], time: number) => {
  const newest = Math.floor(time / READING_S);
  bars.forEach((bar, index) => paint(bar, reading(newest - (bars.length - 1 - index))));
  return reading(newest);
};

/** The history frozen mid-sentence, for still frames. */
export const drawStillLevel = (bars: HTMLElement[]) => drawSpeech(bars, 6.2);
