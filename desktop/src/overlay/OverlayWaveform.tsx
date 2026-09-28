import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import { easeStep, frameRunner } from "./frameClock";
import { audioLevelSource, type LevelSource } from "./levelSource";

// Bar and gap, px. The pill keeps this spacing and takes as many bars as
// fit: a fixed count stretched over the row left the bars twice as far
// apart as they are wide. The bead's ring is a fixed 24 — its bars sit on
// a circle, not a row, so nothing stretches. A compact ring, for a small
// region of the constructor's shells, has fewer and shorter bars: at 30 px
// the full ring's bars nearly meet in the middle.
const BAR_WIDTH = 3;
const BAR_GAP = 3;
const BAR_PITCH = BAR_WIDTH + BAR_GAP;
const RINGS = { full: { count: 24, base: 3, span: 9 }, compact: { count: 16, base: 2, span: 4 } } as const;
type Ring = keyof typeof RINGS;
const MIN_BARS = 12;
const IDLE_LEVEL = 0.04;

function barCount(width: number) {
  // n bars and n-1 gaps fit in the row: n = (width + gap) / pitch.
  return Math.max(MIN_BARS, Math.floor((width + BAR_GAP) / BAR_PITCH));
}

/** Height and glow for one bar, written straight to the node: at this
 *  density a React pass per level event would redraw the whole row. */
function paintBar(bar: HTMLSpanElement | null, level: number, ring: Ring | null, pixel = false) {
  if (!bar) return;
  // Pixel bars hold four heights, so a reading snaps instead of sliding.
  const visualLevel = pixel ? Math.round(Math.sqrt(level) * 3) / 3 : Math.sqrt(level);
  bar.style.height = `${ring ? RINGS[ring].base + visualLevel * RINGS[ring].span : Math.max(5, Math.round(5 + visualLevel * 21))}px`;
  bar.style.setProperty("--bar-glow", `${6 + visualLevel * 8}px`);
  bar.style.setProperty("--bar-glow-opacity", String(0.35 + visualLevel * 0.35));
  const active = level > IDLE_LEVEL ? "true" : "false";
  if (bar.dataset.active !== active) bar.dataset.active = active;
}

/**
 * The recording level over the last moments, one bar per reading. The
 * newest goes in the last place: the pill fills at its right edge and the
 * trace runs left, and the ring puts that place at twelve o'clock with the
 * readings behind it laid out clockwise, so the trace turns clockwise too.
 */
export type WaveformVariant = "bars" | "wave" | "pixel";
type WaveformProps = {
  surfaceRef?: RefObject<HTMLDivElement | null>;
  circular?: boolean;
  compact?: boolean;
  /** `wave` puts the newest reading in the middle and mirrors it outwards. */
  variant?: WaveformVariant;
  source?: LevelSource;
};

export function OverlayWaveform({ surfaceRef, circular = false, compact = false, variant = "bars", source = audioLevelSource }: WaveformProps) {
  const waveRef = useRef<HTMLDivElement>(null);
  const barsRef = useRef<(HTMLSpanElement | null)[]>([]);
  const levelsRef = useRef<number[]>([]);
  const ring: Ring | null = circular ? compact ? "compact" : "full" : null;
  const [count, setCount] = useState(ring ? RINGS[ring].count : MIN_BARS);

  // The row grows and shrinks while recording — the timer slot and the
  // cancel button both animate — so the count follows its width.
  useLayoutEffect(() => {
    const wave = waveRef.current;
    if (circular || !wave) return;
    const observer = new ResizeObserver(() => setCount(barCount(wave.clientWidth)));
    observer.observe(wave);
    setCount(barCount(wave.clientWidth));
    return () => observer.disconnect();
  }, [circular]);

  // Keep the trace across a resize: drop the oldest readings, or pad the
  // far end, so the bars near the newest one never jump. React also clears
  // the ref of a bar it removes but leaves the slot behind.
  // The mirrored wave reads every other of the newest readings from the middle out.
  const reading = (levels: number[], index: number) => variant === "wave"
    ? levels[levels.length - 1 - Math.abs(index - (count - 1) / 2) * 2 | 0] ?? IDLE_LEVEL
    : levels[index];
  useLayoutEffect(() => {
    barsRef.current.length = count;
    const levels = levelsRef.current;
    levels.splice(0, Math.max(0, levels.length - count));
    while (levels.length < count) levels.unshift(IDLE_LEVEL);
    for (let index = 0; index < count; index++) paintBar(barsRef.current[index], reading(levels, index), ring, variant === "pixel");
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `reading` only depends on these
  }, [count, ring, variant]);

  const readingRef = useRef(reading);
  readingRef.current = reading;
  useEffect(() => {
    let energy = 0.08;
    const surface = surfaceRef?.current;
    surface?.style.setProperty("--overlay-energy", String(energy));
    const pixel = variant === "pixel";
    // Readings arrive about 30 times a second. Smooth bars glide to each one on
    // the shared ~60 fps clock; pixel bars snap by design, so they paint per reading.
    const shown: number[] = [];
    const runner = frameRunner((_now, seconds) => {
      const levels = levelsRef.current, step = easeStep(seconds, 0.035);
      for (let index = 0; index < barsRef.current.length; index++) {
        const target = readingRef.current(levels, index);
        const eased = (shown[index] ?? target) + (target - (shown[index] ?? target)) * step;
        // An exponential ease never lands; settle on the reading once it is too close to see.
        shown[index] = Math.abs(target - eased) < 0.001 ? target : eased;
        paintBar(barsRef.current[index], shown[index], ring);
      }
    });
    const stop = source((sample) => {
      const levels = levelsRef.current;
      levels.push(sample);
      levels.splice(0, Math.max(0, levels.length - barsRef.current.length));
      energy = energy * 0.78 + Math.sqrt(sample) * 0.22;
      surface?.style.setProperty("--overlay-energy", String(Math.max(0.08, energy)));
      if (!pixel) { runner.wake(); return; }
      for (let index = 0; index < barsRef.current.length; index++) paintBar(barsRef.current[index], readingRef.current(levels, index), ring, true);
    });
    return () => { stop(); runner.stop(); surface?.style.removeProperty("--overlay-energy"); };
  }, [surfaceRef, ring, variant, source]);

  return <div ref={waveRef} className={`overlay-waveform${circular ? " overlay-waveform--circular" : ""}${variant === "pixel" ? " overlay-waveform--pixel" : ""}`} aria-hidden="true">
    {Array.from({ length: count }, (_, index) => (
      <span key={index} ref={(node) => { barsRef.current[index] = node; }} data-active="false" style={circular
        ? { transform: `translate(-50%, -50%) rotate(${((count - 1 - index) * 360) / count}deg) translateY(calc(-1 * var(--overlay-ring-radius)))` } as CSSProperties
        : undefined} />
    ))}
  </div>;
}
