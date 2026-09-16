import { useEffect, useState, type CSSProperties, type RefObject } from "react";
import { subscribe } from "../bridge/events";

export function OverlayWaveform({ surfaceRef, circular = false }: { surfaceRef: RefObject<HTMLDivElement | null>; circular?: boolean }) {
  const [levels, setLevels] = useState<number[]>(() => Array(24).fill(0.04));
  useEffect(() => {
    let energy = 0.08;
    const surface = surfaceRef.current;
    surface?.style.setProperty("--overlay-energy", String(energy));
    const stop = subscribe<{ level?: number }>("audio-level", (payload) => {
      const level = payload?.level;
      const sample = Math.max(0, Math.min(1, typeof level === "number" && Number.isFinite(level) ? level : 0));
      setLevels((current) => [...current.slice(1), sample]);
      energy = energy * 0.78 + Math.sqrt(sample) * 0.22;
      surface?.style.setProperty("--overlay-energy", String(Math.max(0.08, energy)));
    });
    return () => { stop(); surface?.style.removeProperty("--overlay-energy"); };
  }, [surfaceRef]);
  return <div className={`overlay-waveform${circular ? " overlay-waveform--circular" : ""}`} aria-hidden="true">
    {levels.map((level, index) => {
      const visualLevel = Math.sqrt(level);
      return <span key={index} data-active={level > 0.04} style={{
        height: `${circular ? 3 + visualLevel * 9 : Math.max(5, Math.round(5 + visualLevel * 21))}px`,
        ...(circular ? { transform: `translate(-50%, -50%) rotate(${index * 15}deg) translateY(calc(-1 * var(--overlay-ring-radius)))` } : {}),
        "--bar-glow": `${6 + visualLevel * 8}px`,
        "--bar-glow-opacity": 0.35 + visualLevel * 0.35,
      } as CSSProperties} />;
    })}
  </div>;
}
