/**
 * Ported from voice-glow 0.2.0 (MIT).
 * Copyright (c) 2026 Jakub Antalik
 * https://github.com/Jakubantalik/Libraries.dev/tree/main/packages/voice-glow
 *
 * Sotto keeps this construction and drives it from existing `audio-level`
 * events instead of opening a Web Audio graph.
 */

import { parseRgb } from '../../color';

/** `rgb(r, g, b)` for the stylesheet, or null if unparseable. */
export function toRgb(color: string): string | null {
  const t = parseRgb(color);
  return t ? `rgb(${t[0]}, ${t[1]}, ${t[2]})` : null;
}

/** `r, g, b` for the canvas (dropped into rgba()), or null. */
export function toTriple(color: string): string | null {
  const t = parseRgb(color);
  return t ? `${t[0]}, ${t[1]}, ${t[2]}` : null;
}
