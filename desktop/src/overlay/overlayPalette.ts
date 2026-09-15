import type { CSSProperties } from "react";
import { resolveAccent } from "../accent";
import type { OverlayPreferences } from "./overlayPreferences";

const PALETTES = {
  coal: [45, 0.08], graphite: [0, 0], lagoon: [195, 0.1], amber: [70, 0.14], violet: [295, 0.14],
} as const;
const ACCENTS = { "#e68a3d": 55, "#5b8def": 260, "#3dc97c": 155, "#9b75ef": 295 } as const;

// Fixed lightness separates the dark surface, shell and waveform at every hue.
// CSS retains OKLCH so the webview handles its display's color gamut.
export function paletteVariables(hue: number, chroma: number): CSSProperties {
  const color = (lightness: number, saturation = 1, alpha = 1) =>
    `oklch(${lightness} ${chroma * saturation} ${hue} / ${alpha})`;
  return {
    "--overlay-surface": color(0.18, 0.12, 0.97),
    "--overlay-shell-top": color(0.43, 0.7, 0.9),
    "--overlay-shell-bottom": color(0.3, 0.5, 0.94),
    "--overlay-edge": color(0.8, 0.8),
    "--overlay-wave-top": color(0.9, 0.6),
    "--overlay-wave-mid": color(0.76),
    "--overlay-wave-bottom": color(0.62),
  } as CSSProperties;
}

export function overlayPalette(preferences: OverlayPreferences, accent: unknown): CSSProperties {
  const { palette, palette_hue, palette_chroma } = preferences;
  const [hue, chroma] = palette === "custom" ? [palette_hue, palette_chroma]
    : palette === "accent" ? [ACCENTS[resolveAccent(accent)], 0.14] : PALETTES[palette];
  return paletteVariables(hue, chroma);
}
