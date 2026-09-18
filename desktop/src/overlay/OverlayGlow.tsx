import { useEffect, useRef } from "react";
import { subscribe } from "../bridge/events";
import {
  applyGlowFrame,
  FLOW_PX_S,
  glowFrame,
  PROCESSING_LEVEL,
  PROCESSING_PERIOD_S,
  stepEnvelope,
  type GlowMode,
} from "./glowFrame";

export function OverlayGlow({ mode }: { mode: GlowMode }) {
  const ref = useRef<HTMLDivElement>(null);
  const modeRef = useRef(mode);
  modeRef.current = mode;
  const levelRef = useRef(0);

  useEffect(() => {
    const stop = subscribe<{ level?: number }>("audio-level", (payload) => {
      const level = payload?.level;
      levelRef.current = typeof level === "number" && Number.isFinite(level) ? Math.max(0, Math.min(1, level)) : 0;
    });
    return stop;
  }, []);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let envelope = 0.12;
    let phase = 0;
    let travel = 0;
    let last = performance.now();
    let raf = 0;

    const tick = (now: number) => {
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      const modeNow = modeRef.current;
      const sample = modeNow === "listen" ? levelRef.current : modeNow === "process" ? PROCESSING_LEVEL : 0;
      envelope = stepEnvelope(envelope, sample, dt);
      if (!reducedMotion && modeNow === "listen") phase += dt * FLOW_PX_S * envelope;
      if (!reducedMotion && modeNow === "process") travel += dt / PROCESSING_PERIOD_S;
      const breathe = 0.5 + 0.5 * Math.sin((now / 1000 / 5.2) * Math.PI * 2);
      applyGlowFrame(el, glowFrame({ envelope, phase, travel, breathe, mode: modeNow, reducedMotion }));
      raf = window.requestAnimationFrame(tick);
    };
    raf = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(raf);
  }, []);

  return (
    <div className="overlay-beam" ref={ref} aria-hidden="true">
      <div className="overlay-beam__inner" />
      <div className="overlay-beam__bloom" />
    </div>
  );
}
