// Bottom-edge voice wash: seven colored lobes whose height follows the
// recording level. Geometry is adapted from the MIT-licensed voice-glow
// construction (https://libraries.dev/voice); Sotto drives it from the
// existing `audio-level` events instead of opening a Web Audio graph.

export const GLOW_LOBES = [
  { x: 0, w: 74, h: 46 },
  { x: -36, w: 54, h: 40 },
  { x: 36, w: 54, h: 40 },
  { x: -72, w: 48, h: 32 },
  { x: 72, w: 48, h: 32 },
  { x: -108, w: 42, h: 26 },
  { x: 108, w: 42, h: 26 },
] as const;

export const LOBE_SPACING = 36;
export const LOBE_SPAN = LOBE_SPACING * GLOW_LOBES.length;

export const ATTACK_S = 0.325;
export const RELEASE_S = 0.86;
export const GATE = 0.015;
export const IDLE = 0.18;
export const PROCESSING_LEVEL = 0.55;
export const FLOW_PX_S = 48;
export const PROCESSING_PERIOD_S = 1.1;

export type GlowMode = "listen" | "process" | "idle";

export type GlowFrame = {
  h: number;
  w: number;
  glow: number;
  bh: number;
  cx: number;
  mw: number;
  xs: number[];
  ls: number[];
};

export function wrapOffset(x: number, span: number): number {
  const half = span / 2;
  return ((((x + half) % span) + span) % span) - half;
}

export function stepEnvelope(envelope: number, sample: number, dt: number): number {
  const gated = sample < GATE ? 0 : Math.max(0, Math.min(1, sample));
  const tau = gated > envelope ? ATTACK_S : RELEASE_S;
  return envelope + (gated - envelope) * (1 - Math.exp(-Math.max(0, dt) / tau));
}

export function glowFrame(input: {
  envelope: number;
  phase: number;
  travel: number;
  breathe: number;
  mode: GlowMode;
  reducedMotion: boolean;
}): GlowFrame {
  const { envelope, phase, mode, reducedMotion } = input;
  const breathe = reducedMotion ? 1 : input.breathe;
  const idlePresence = IDLE * (0.7 + 0.3 * breathe);
  const level = Math.max(idlePresence, envelope);
  const processing = mode === "process";
  const travel = processing && !reducedMotion ? input.travel : 0;
  const gather = processing ? 0.72 : 0;
  const cx = processing ? Math.sin(travel * Math.PI) * 88 : 0;
  return {
    h: 0.78 + level * 0.42,
    w: 1 + envelope * 0.08,
    glow: mode === "idle" ? idlePresence : IDLE + envelope * (1 - IDLE),
    bh: envelope * 28,
    cx,
    mw: processing ? 0.58 : 1,
    xs: GLOW_LOBES.map((lobe) => wrapOffset(lobe.x + phase, LOBE_SPAN) * (1 - gather)),
    ls: GLOW_LOBES.map((lobe) => {
      const dist = Math.min(1, Math.abs(lobe.x) / 108);
      return 0.7 + envelope * (0.5 - dist * 0.18);
    }),
  };
}

export function applyGlowFrame(el: HTMLElement, frame: GlowFrame): void {
  el.style.setProperty("--vb-h", frame.h.toFixed(3));
  el.style.setProperty("--vb-w", frame.w.toFixed(3));
  el.style.setProperty("--vb-glow", frame.glow.toFixed(3));
  el.style.setProperty("--vb-bh", `${frame.bh.toFixed(1)}px`);
  el.style.setProperty("--vb-cx", `${frame.cx.toFixed(1)}px`);
  el.style.setProperty("--vb-mw", frame.mw.toFixed(3));
  for (let i = 0; i < frame.xs.length; i++) {
    el.style.setProperty(`--vb-x${i}`, `${frame.xs[i].toFixed(1)}px`);
    el.style.setProperty(`--vb-l${i}`, frame.ls[i].toFixed(3));
  }
}
