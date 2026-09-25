import { describe, expect, it } from "vitest";
import { DEFAULT_EDGE_OFFSET, overlayLayout, overlayPreferences } from "./overlayPreferences";
import { overlayPalette, paletteVariables } from "./overlayPalette";
import { accentVariables, resolveAccent } from "../accent";

describe("overlay preferences", () => {
  it("normalizes older and hand-edited configs without losing valid fields", () => {
    const defaults = overlayPreferences(undefined);
    expect(defaults.palette).toBe("graphite");
    expect(defaults.edge_offset).toBe(25);
    expect(overlayPreferences({ edge_offset: 20 }).edge_offset).toBe(20);
    expect(overlayPreferences(false)).toEqual(defaults);
    expect(overlayPreferences({ form: "bead", size: "xl", palette: "wrong", edge_offset: -1, palette_hue: 360, palette_chroma: Infinity }))
      .toEqual({ ...defaults, form: "bead" });
    expect(overlayPreferences({ form: "glow" }).form).toBe("glow");
    expect(overlayPreferences({ edge_offset: 0.5 }).edge_offset).toBe(DEFAULT_EDGE_OFFSET);
    expect(overlayPreferences({ palette_hue: 359.9, palette_chroma: 0.2, edge_offset: 512 })).toMatchObject({ palette_hue: 359.9, palette_chroma: 0.2, edge_offset: 512 });
    expect(defaults.show_timer).toBe(true);
    expect(overlayPreferences({ show_timer: false }).show_timer).toBe(false);
    expect(overlayPreferences({ show_timer: "no" }).show_timer).toBe(true);
  });
  it("keeps a bead during streaming and restores it after a warning", () => {
    expect(overlayLayout("bead", true, false)).toBe("bead");
    expect(overlayLayout("bead", true, true)).toBe("pill");
    expect(overlayLayout("bead", false, false)).toBe("bead");
    expect(overlayLayout("pill", true, false)).toBe("streaming");
    expect(overlayLayout("glow", true, false)).toBe("glow");
    expect(overlayLayout("glow", true, true)).toBe("glow");
    expect(overlayLayout("glow", false, false)).toBe("glow");
  });
  it("accepts any hex interface colour and refuses anything else", () => {
    expect(resolveAccent("#0A0B0C")).toBe("#0a0b0c");
    expect(resolveAccent("#9b75ef")).toBe("#9b75ef");
    expect(resolveAccent("red")).toBe("#e68a3d");
    expect(resolveAccent(undefined)).toBe("#e68a3d");
  });
  it("writes readable text on a light and on a dark interface colour", () => {
    // The ink is what is printed on the accent: it has to flip, or a navy
    // button gets near-black text on it.
    const ink = (color: string) => parseInt(accentVariables(color)["--accent-ink"].slice(1, 3), 16);
    expect(ink("#f2e14a")).toBeLessThan(60);
    expect(ink("#102040")).toBeGreaterThan(195);
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
  it("defaults to neutral graphite and preserves an explicit copper choice", () => {
    const neutral = overlayPalette(overlayPreferences(undefined));
    expect(Object.values(neutral).every((value) => /^oklch\([\d.]+ 0 /.test(String(value)))).toBe(true);
    expect(overlayPreferences({ palette: "copper" }).palette).toBe("copper");
    for (const retired of ["accent", "coal", "amber"]) {
      expect(overlayPreferences({ palette: retired }).palette).toBe("copper");
    }
  });
});
