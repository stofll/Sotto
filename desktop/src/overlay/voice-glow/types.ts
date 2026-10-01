/**
 * Ported from voice-glow 0.2.0 (MIT).
 * Copyright (c) 2026 Jakub Antalik
 * https://github.com/Jakubantalik/Libraries.dev/tree/main/packages/voice-glow
 *
 * Sotto keeps this construction and drives it from existing `audio-level`
 * events instead of opening a Web Audio graph.
 */

/**
 * Host preset — retunes the glow's geometry for the element it wraps
 * - 'default': a chat input or card, ~350px wide
 * - 'pill': a small recording pill, ~150×44 — glow pulled in, shallower bend
 * - 'mobile': the bottom of a phone screen — wider range, taller rise
 *
 * A preset only supplies defaults; every geometry prop still overrides it.
 */
export type VoiceBeamType = 'default' | 'pill' | 'mobile';

/**
 * Color variant for the beam — the same eight palettes as border-beam
 * - 'colorful': Full spectrum, Siri-like (default)
 * - 'mono': Monochromatic grayscale
 * - 'ocean': Blue and purple tones
 * - 'sunset': Warm orange, yellow, and red tones
 * - 'forest': Green and teal tones
 * - 'candy': Pink and magenta tones
 * - 'ice': Cyan and pale blue tones
 * - 'gold': Amber and yellow tones
 */
export type VoiceBeamColorVariant =
  | 'colorful'
  | 'mono'
  | 'ocean'
  | 'sunset'
  | 'forest'
  | 'candy'
  | 'ice'
  | 'gold';

/**
 * Theme color configuration
 */
export interface VoiceThemeColors {
  strokeOpacity: number;
  innerOpacity: number;
  bloomOpacity: number;
  innerShadow: string;
  saturation: number;
  brightness: number;
  /** Optional per-theme hue drift defaults; fall back to 24° / 12 s / 0°. */
  hueRange?: number;
  hueDuration?: number;
  hueBase?: number;
  /** Optional per-theme overall strength (falls back to 1). */
  strength?: number;
  /** Optional per-theme band strength for types that do not set their own. */
  bandStrength?: number;
}
