import { describe, expect, it } from "vitest";
import { ringClip, ringHoleSize } from "./shellShapes";


describe("ringClip", () => {
  it("cuts an even-odd hole of the given share out of the outer circle", () => {
    const clip = ringClip(80, 0.5);
    expect(clip.startsWith("path(evenodd,")).toBe(true);
    expect(clip).toContain("M0 40a40 40 0 1 0 80 0a40 40 0 1 0 -80 0Z");
    expect(clip).toContain("M20 40a20 20 0 1 0 40 0a20 20 0 1 0 -40 0Z");
  });

  it("sizes the hole from the same share", () => {
    expect(ringHoleSize(72)).toBe(40);
    expect(ringHoleSize(80, 0.5)).toBe(40);
  });
});
