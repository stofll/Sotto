import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import { subscribe } from "../bridge/events";

// Bar and gap, px. The pill keeps this spacing and takes as many bars as
// fit: a fixed count stretched over the row left the bars twice as far
// apart as they are wide. The bead's ring is a fixed 24 — its bars sit on
// a circle, not a row, so nothing stretches.
const BAR_WIDTH = 3;
const BAR_GAP = 3;
const BAR_PITCH = BAR_WIDTH + BAR_GAP;
const RING_BARS = 24;
const MIN_BARS = 12;
const IDLE_LEVEL = 0.04;

function barCount(width: number) {
  // n bars and n-1 gaps fit in the row: n = (width + gap) / pitch.
  return Math.max(MIN_BARS, Math.floor((width + BAR_GAP) / BAR_PITCH));
}

/** Height and glow for one bar, written straight to the node: at this
 *  density a React pass per level event would redraw the whole row. */
function paintBar(bar: HTMLSpanElement | null, level: number, circular: boolean) {
  if (!bar) return;
  const visualLevel = Math.sqrt(level);
  bar.style.height = `${circular ? 3 + visualLevel * 9 : Math.max(5, Math.round(5 + visualLevel * 21))}px`;
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
export function OverlayWaveform({ surfaceRef, circular = false }: { surfaceRef: RefObject<HTMLDivElement | null>; circular?: boolean }) {
  const waveRef = useRef<HTMLDivElement>(null);
  const barsRef = useRef<(HTMLSpanElement | null)[]>([]);
  const levelsRef = useRef<number[]>([]);
  const [count, setCount] = useState(circular ? RING_BARS : MIN_BARS);

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
  useLayoutEffect(() => {
    barsRef.current.length = count;
    const levels = levelsRef.current;
    levels.splice(0, Math.max(0, levels.length - count));
    while (levels.length < count) levels.unshift(IDLE_LEVEL);
    for (let index = 0; index < count; index++) paintBar(barsRef.current[index], levels[index], circular);
  }, [count, circular]);

  useEffect(() => {
    let energy = 0.08;
    const surface = surfaceRef.current;
    surface?.style.setProperty("--overlay-energy", String(energy));
    const stop = subscribe<{ level?: number }>("audio-level", (payload) => {
      const level = payload?.level;
      const sample = Math.max(0, Math.min(1, typeof level === "number" && Number.isFinite(level) ? level : 0));
      const levels = levelsRef.current;
      levels.push(sample);
      levels.splice(0, Math.max(0, levels.length - barsRef.current.length));
      energy = energy * 0.78 + Math.sqrt(sample) * 0.22;
      surface?.style.setProperty("--overlay-energy", String(Math.max(0.08, energy)));
      for (let index = 0; index < levels.length; index++) paintBar(barsRef.current[index], levels[index], circular);
    });
    return () => { stop(); surface?.style.removeProperty("--overlay-energy"); };
  }, [surfaceRef, circular]);

  return <div ref={waveRef} className={`overlay-waveform${circular ? " overlay-waveform--circular" : ""}`} aria-hidden="true">
    {Array.from({ length: count }, (_, index) => (
      <span key={index} ref={(node) => { barsRef.current[index] = node; }} data-active="false" style={circular
        ? { transform: `translate(-50%, -50%) rotate(${((count - 1 - index) * 360) / count}deg) translateY(calc(-1 * var(--overlay-ring-radius)))` } as CSSProperties
        : undefined} />
    ))}
  </div>;
}
