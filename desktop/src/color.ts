/**
 * Ported from voice-glow 0.2.0 (MIT).
 * Copyright (c) 2026 Jakub Antalik
 * https://github.com/Jakubantalik/Libraries.dev/tree/main/packages/voice-glow
 */

/** Parse #rgb, #rrggbb, rgb() or rgba() into RGB channels, ignoring alpha. */
export function parseRgb(color: string): [number, number, number] | null {
  const c = color.trim();
  const hex = c.match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (hex) {
    const h = hex[1].length === 3 ? hex[1].split('').map((ch) => ch + ch).join('') : hex[1];
    return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
  }
  const fn = c.match(/^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)/i);
  if (fn) return [Math.round(+fn[1]), Math.round(+fn[2]), Math.round(+fn[3])];
  return null;
}
