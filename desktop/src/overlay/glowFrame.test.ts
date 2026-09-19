import { describe, expect, it } from "vitest";
import { voiceLobes, LOBE_SPAN } from "./voice-glow/styles";
import { follow, shape, wrapX, processingPosition, cornerLift } from "./voice-glow/voiceDriver";

describe("overlay glow", () => {
  it("wraps lobe travel onto a ring around the centre", () => {
    expect(wrapX(0, LOBE_SPAN)).toBe(0);
    expect(wrapX(LOBE_SPAN, LOBE_SPAN)).toBe(0);
    expect(wrapX(LOBE_SPAN / 2, LOBE_SPAN)).toBeCloseTo(-LOBE_SPAN / 2);
    expect(wrapX(-LOBE_SPAN / 2, LOBE_SPAN)).toBeCloseTo(-LOBE_SPAN / 2);
  });

  // The overlay's own attack and release (see TUNING in OverlayGlow.tsx).
  it("rises faster than it falls", () => {
    const up = follow(0, 1, 0.2, 0.14, 0.55);
    const down = follow(1, 0, 0.2, 0.14, 0.55);
    expect(up).toBeGreaterThan(0.4);
    expect(1 - down).toBeLessThan(up);
    expect(follow(0.5, 0, 0.2, 0.14, 0.55)).toBeLessThan(0.5);
  });

  it("gates quiet noise then saturates a shout", () => {
    expect(shape(0, 0.015)).toBe(0);
    expect(shape(0.01, 0.015)).toBe(0);
    expect(shape(0.08, 0.015)).toBeGreaterThan(0.15);
    expect(shape(1, 0.015)).toBeGreaterThan(0.9);
  });

  it("keeps the seven-lobe ring from the voice-glow construction", () => {
    expect(voiceLobes).toHaveLength(7);
    expect(LOBE_SPAN).toBe(36 * 7);
  });
});

describe("processing sweep", () => {
  it("travels continuously between both ends and repeats", () => {
    expect(processingPosition(0)).toBe(-1);
    expect(processingPosition(0.5)).toBeCloseTo(0);
    expect(processingPosition(1)).toBe(1);
    expect(processingPosition(1.5)).toBeCloseTo(0);
    expect(processingPosition(2)).toBe(-1);
    for (let phase = 0; phase < 2; phase += 0.01) {
      expect(processingPosition(phase + 2)).toBeCloseTo(processingPosition(phase));
    }
  });

  it("reverses with zero velocity and continuous bounded acceleration", () => {
    const dt = 0.001;
    const velocity = (t: number) => (processingPosition(t + dt) - processingPosition(t - dt)) / (2 * dt);
    const acceleration = (t: number) => (processingPosition(t + dt) - 2 * processingPosition(t) + processingPosition(t - dt)) / (dt * dt);
    for (const turn of [1, 2, 3]) {
      expect(Math.abs(velocity(turn))).toBeLessThan(0.001);
      expect(velocity(turn - dt) * velocity(turn + dt)).toBeLessThan(0);
      expect(Math.abs(acceleration(turn))).toBeLessThan(10);
      expect(acceleration(turn - dt)).toBeCloseTo(acceleration(turn + dt), 3);
    }
  });
});

describe("processing corner lift", () => {
  it("is symmetric, bounded and rises monotonically toward either edge", () => {
    for (const width of [240, 308, 400]) {
      for (const reach of [0, 30, 42]) {
        let previous = 24;
        for (let x = -20; x <= width / 2; x += 0.25) {
          const lift = cornerLift(x, width, 24, reach);
          expect(lift).toBeGreaterThanOrEqual(0);
          expect(lift).toBeLessThanOrEqual(24);
          expect(lift).toBeLessThanOrEqual(previous);
          expect(lift).toBeCloseTo(cornerLift(width - x, width, 24, reach));
          previous = lift;
        }
      }
    }
    expect(cornerLift(0, 308, 0)).toBe(0);
  });

  it("joins the flat and clamped sections without a vertical kick", () => {
    const dt = 0.001;
    for (const reach of [0, 30, 42]) {
      for (const edge of [reach, reach + 24, 308 - reach - 24, 308 - reach]) {
        const before = (cornerLift(edge, 308, 24, reach) - cornerLift(edge - dt, 308, 24, reach)) / dt;
        const after = (cornerLift(edge + dt, 308, 24, reach) - cornerLift(edge, 308, 24, reach)) / dt;
        expect(Math.abs(before)).toBeLessThan(0.001);
        expect(Math.abs(after)).toBeLessThan(0.001);
      }
    }
  });
});
