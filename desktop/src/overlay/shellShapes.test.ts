import { describe, expect, it } from "vitest";
import { ringClip, ringHoleSize, tornEdgeClip } from "./shellShapes";

const points = (clip: string) => clip.replace(/^polygon\(|\)$/g, "").split(",").map((point) => point.split(" ").map(parseFloat));

describe("tornEdgeClip", () => {
  it("keeps the top straight and alternates the bottom between the edge and the tooth depth", () => {
    const list = points(tornEdgeClip(220, 156));
    expect(list.slice(0, 2)).toEqual([[0, 0], [220, 0]]);
    const bottom = list.slice(2);
    expect(bottom[0]).toEqual([220, 156]);
    expect(bottom[bottom.length - 1][0]).toBe(0);
    expect(new Set(bottom.map(([, y]) => y))).toEqual(new Set([156, 150]));
    bottom.forEach(([, y], index) => expect(y).toBe(index % 2 ? 150 : 156));
  });

  it("spaces the teeth evenly even when the width is not a multiple of a tooth", () => {
    const xs = points(tornEdgeClip(197, 100)).slice(2).map(([x]) => x);
    const gaps = xs.slice(1).map((x, index) => xs[index] - x);
    expect(Math.max(...gaps) - Math.min(...gaps)).toBeLessThan(0.02);
    expect(xs[0]).toBe(197);
    expect(xs[xs.length - 1]).toBe(0);
  });

  it("still tears a very narrow strip", () => {
    expect(points(tornEdgeClip(4, 40)).length).toBeGreaterThanOrEqual(5);
  });
});

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
