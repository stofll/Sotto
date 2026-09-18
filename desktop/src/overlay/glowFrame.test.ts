import { describe, expect, it } from "vitest";
import { GLOW_LOBES, glowFrame, LOBE_SPAN, stepEnvelope, wrapOffset } from "./glowFrame";

describe("overlay glow", () => {
  it("wraps lobe travel onto a ring around the centre", () => {
    expect(wrapOffset(0, LOBE_SPAN)).toBe(0);
    expect(wrapOffset(LOBE_SPAN, LOBE_SPAN)).toBe(0);
    expect(wrapOffset(LOBE_SPAN / 2, LOBE_SPAN)).toBeCloseTo(-LOBE_SPAN / 2);
    expect(wrapOffset(-LOBE_SPAN / 2, LOBE_SPAN)).toBeCloseTo(-LOBE_SPAN / 2);
  });

  it("rises faster than it falls", () => {
    const up = stepEnvelope(0, 1, 0.2);
    const down = stepEnvelope(1, 0, 0.2);
    expect(up).toBeGreaterThan(0.4);
    expect(1 - down).toBeLessThan(up);
    expect(stepEnvelope(0.5, 0.01, 0.2)).toBeLessThan(0.5);
  });

  it("gathers into a travelling beam while processing", () => {
    const listen = glowFrame({ envelope: 0.8, phase: 0, travel: 0, breathe: 1, mode: "listen", reducedMotion: false });
    const process = glowFrame({ envelope: 0.8, phase: 0, travel: 0.5, breathe: 1, mode: "process", reducedMotion: false });
    expect(process.mw).toBeLessThan(listen.mw);
    expect(Math.abs(process.cx)).toBeGreaterThan(0);
    expect(Math.max(...process.xs.map(Math.abs))).toBeLessThan(Math.max(...listen.xs.map(Math.abs)));
    expect(listen.xs).toHaveLength(GLOW_LOBES.length);
  });

  it("holds geometry still when motion is reduced", () => {
    const a = glowFrame({ envelope: 0.4, phase: 40, travel: 0.8, breathe: 0, mode: "process", reducedMotion: true });
    const b = glowFrame({ envelope: 0.4, phase: 40, travel: 1.6, breathe: 1, mode: "process", reducedMotion: true });
    expect(a.cx).toBe(0);
    expect(a.xs).toEqual(b.xs);
    expect(a.glow).toBeCloseTo(b.glow);
  });
});
