import { describe, expect, it } from "vitest";
import { voiceLobes, LOBE_SPAN } from "./voice-glow/styles";
import { follow, shape, wrapX } from "./voice-glow/voiceDriver";

describe("overlay glow", () => {
  it("wraps lobe travel onto a ring around the centre", () => {
    expect(wrapX(0, LOBE_SPAN)).toBe(0);
    expect(wrapX(LOBE_SPAN, LOBE_SPAN)).toBe(0);
    expect(wrapX(LOBE_SPAN / 2, LOBE_SPAN)).toBeCloseTo(-LOBE_SPAN / 2);
    expect(wrapX(-LOBE_SPAN / 2, LOBE_SPAN)).toBeCloseTo(-LOBE_SPAN / 2);
  });

  it("rises faster than it falls", () => {
    const up = follow(0, 1, 0.2, 0.325, 0.86);
    const down = follow(1, 0, 0.2, 0.325, 0.86);
    expect(up).toBeGreaterThan(0.4);
    expect(1 - down).toBeLessThan(up);
    expect(follow(0.5, 0, 0.2, 0.325, 0.86)).toBeLessThan(0.5);
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
