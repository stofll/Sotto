/**
 * Ported from voice-glow 0.2.0 (MIT).
 * Copyright (c) 2026 Jakub Antalik
 * https://github.com/Jakubantalik/Libraries.dev/tree/main/packages/voice-glow
 *
 * Sotto keeps this construction and drives it from existing `audio-level`
 * events instead of opening a Web Audio graph.
 */

/** Geometry used by Sotto's voice glow. */
export interface VoiceGeometry {
  /** Size of the whole effect (see the `scale` prop). */
  scale: number;
  /** Bloom blur multiplier (see the `glowSize` prop). */
  glowSize: number;
  /** Stroke opacity multiplier on the theme preset. */
  strokeOpacity: number;
  spread: number;
  bandStrength: number;
  bandWidth: number;
  bandPosition: number;
  bandCurve: number;
  bandSpread: number;
  bandSkew: number;
  bandOffset: number;
  /** How far the band's ends rise again toward the corners, as a fraction of its peak. */
  bandTail: number;
  /** Where the tail's rise starts, as a share of the centre-to-edge distance; it always reaches full lift at the corner. */
  bandTailPosition: number;
  /** Exponent of the rise: 1 a straight ramp, higher a hook that stays low then whips up at the corner. */
  bandTailCurve: number;
  /** Px the band runs past each side of the element, so the hook is cropped by the component. */
  bandTailOverflow: number;
  bandAberration: number;
  distortion: number;
  distortionDetail: number;
  glowWidth: number;
  glowHeight: number;
  lobeSpacing: number;
  rangeWidth: number;
  rangeHeight: number;
  softness: number;
  coreSize: number;
  strokeScale: number;
  innerScale: number;
  innerHeight: number;
  bloomScale: number;
  bloomHeight: number;
}

/**
 * The tuned defaults for the original ~350px chat input.
 */
export const voiceDefaults: VoiceGeometry = {
  scale: 1,
  glowSize: 1,
  strokeOpacity: 1,
  spread: 1.05,
  bandStrength: 1.55,
  bandWidth: 2.15,
  bandPosition: 0.35,
  bandCurve: 1.75,
  bandSpread: 0.87,
  bandSkew: 0.12,
  bandOffset: -27,
  bandTail: 0.59,
  bandTailPosition: 0.67,
  bandTailCurve: 2.4,
  bandTailOverflow: 15,
  bandAberration: 0.89,
  distortion: 0.62,
  distortionDetail: 2.3,
  glowWidth: 0.65,
  glowHeight: 1.25,
  lobeSpacing: 0.85,
  rangeWidth: 0.75,
  rangeHeight: 1,
  softness: 1.07,
  coreSize: 1,
  strokeScale: 1,
  innerScale: 1,
  innerHeight: 1,
  bloomScale: 1,
  bloomHeight: 1,
};
