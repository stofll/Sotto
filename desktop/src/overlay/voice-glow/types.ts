/**
 * Ported from voice-glow 0.2.0 (MIT).
 * Copyright (c) 2026 Jakub Antalik
 * https://github.com/Jakubantalik/Libraries.dev/tree/main/packages/voice-glow
 *
 * Sotto keeps this construction and drives it from existing `audio-level`
 * events instead of opening a Web Audio graph.
 */

/** Theme colors used by Sotto's voice glow. */
export interface VoiceThemeColors {
  strokeOpacity: number;
  innerOpacity: number;
  bloomOpacity: number;
  innerShadow: string;
}
