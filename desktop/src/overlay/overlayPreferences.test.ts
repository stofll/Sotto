import { describe, expect, it } from "vitest";
import { overlayLayout, overlayPreferences } from "./overlayPreferences";
import { overlayPalette, paletteVariables } from "./overlayPalette";
import { resolveAccent } from "../accent";

describe("overlay preferences", () => {
  it("normalizes older and hand-edited configs without losing valid fields", () => {
    const defaults = overlayPreferences(undefined);
    expect(overlayPreferences(false)).toEqual(defaults);
    expect(overlayPreferences({ form: "bead", size: "xl", palette: "wrong", edge_offset: -1, palette_hue: 360, palette_chroma: Infinity }))
      .toEqual({ ...defaults, form: "bead" });
    expect(overlayPreferences({ edge_offset: 0.5 }).edge_offset).toBe(96);
    expect(overlayPreferences({ palette_hue: 359.9, palette_chroma: 0.2, edge_offset: 512 })).toMatchObject({ palette_hue: 359.9, palette_chroma: 0.2, edge_offset: 512 });
  });
  it("keeps a bead during streaming and restores it after a warning", () => {
    expect(overlayLayout("bead", true, false)).toBe("bead");
    expect(overlayLayout("bead", true, true)).toBe("pill");
    expect(overlayLayout("bead", false, false)).toBe("bead");
    expect(overlayLayout("pill", true, false)).toBe("streaming");
  });
  it("falls back for an unknown accent", () => {
    expect(resolveAccent("#000000")).toBe("#e68a3d");
    expect(resolveAccent("#9b75ef")).toBe("#9b75ef");
  });
});

describe("overlay palette", () => {
  it.each([0, 55, 155, 195, 260, 295, 359])("preserves lightness separation at hue %s", (hue) => {
    const colors = paletteVariables(hue, 0.14) as Record<string, string>;
    const lightness = (name: string) => Number(colors[name].match(/oklch\(([\d.]+)/)![1]);
    expect(lightness("--overlay-shell-bottom")).toBeGreaterThan(lightness("--overlay-surface"));
    expect(lightness("--overlay-edge")).toBeGreaterThan(lightness("--overlay-shell-top"));
    expect(lightness("--overlay-wave-top")).toBeGreaterThan(lightness("--overlay-wave-mid"));
    expect(lightness("--overlay-wave-mid")).toBeGreaterThan(lightness("--overlay-wave-bottom"));
  });
  it("graphite is neutral and custom ignores the app accent", () => {
    const neutral = overlayPalette(overlayPreferences({ palette: "graphite" }), "#e68a3d");
    expect(Object.values(neutral).every((value) => /^oklch\([\d.]+ 0 /.test(String(value)))).toBe(true);
    const custom = overlayPreferences({ palette: "custom", palette_hue: 120, palette_chroma: 0.1 });
    expect(overlayPalette(custom, "#e68a3d")).toEqual(overlayPalette(custom, "#5b8def"));
    expect(overlayPalette(overlayPreferences({ palette: "accent" }), "#e68a3d"))
      .not.toEqual(overlayPalette(overlayPreferences({ palette: "accent" }), "#5b8def"));
  });
});
