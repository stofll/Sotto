import { describe, expect, it } from "vitest";
import { notchFit } from "./useOverlayNotch";

describe("notchFit", () => {
  it("keeps a real cut-out", () => {
    expect(notchFit({ width: 186, height: 38 })).toEqual({ width: 186, height: 38 });
  });

  it("treats no answer and damaged answers as no notch", () => {
    for (const raw of [null, undefined, "x", 7, {}, { width: 186 }, { width: 0, height: 38 }, { width: -4, height: 38 }, { width: Number.NaN, height: 38 }, { width: "186", height: 38 }]) {
      expect(notchFit(raw), String(JSON.stringify(raw))).toBeNull();
    }
  });
});
