import { useEffect, useId, useMemo, useRef, type CSSProperties } from "react";
import { subscribe } from "../bridge/events";
import { themePresets, generateVoiceBeamCSS } from "./voice-glow/styles";
import { resolveVoiceDefaults, resolveVoiceStyle } from "./voice-glow/presets";
import { registerVoiceInstance, type VoiceDriverConfig } from "./voice-glow/voiceDriver";
import type { OverlayPreferences } from "./overlayPreferences";

export type GlowMode = "listen" | "process" | "idle";

const BAND_COLORS = { core: "255, 255, 255", above: "255, 70, 80", mid: "90, 255, 150", below: "80, 140, 255" } as const;
const THEME = "dark" as const;
const DEFAULTS = resolveVoiceDefaults("default", THEME);
const PRESET = themePresets[THEME];
const TYPE_STYLE = resolveVoiceStyle("default", THEME);
const SCALE = DEFAULTS.scale;
const BORDER_WIDTH = 1;
const DISTORTION_ON = DEFAULTS.distortion > 0;
const CANVAS_FILTER = (() => {
  if (typeof document === "undefined") return true;
  const ctx = document.createElement("canvas").getContext("2d");
  return !!ctx && typeof (ctx as { filter?: unknown }).filter === "string";
})();

// Sotto's own tuning over the library's `default` type. That type was
// authored for a ~350x52 chat input driven by a Web Audio graph; the
// overlay is a 400x104 card fed by `audio-level` at ~30 Hz, and the glow
// this port replaced rose higher and burned brighter than the port did.
const TUNING = {
  // Response, the pair everything else multiplies. The library's 0.325 s
  // attack is slower than a syllable, so the level never approached its
  // peak and the card read dim and late; short enough to answer one, and
  // the light starts pumping instead of breathing. These two are the
  // settled middle — fast enough to follow speech, slow enough to hold a
  // phrase together.
  attack: 0.14,
  release: 0.55,
  // The rise: how far the light climbs the card, and how far its ceiling
  // humps up at the centre. Both are multiplied by the level, which now
  // reaches its peaks, so they carry less than the doubled host height
  // would suggest on its own.
  reach: 1.5,
  bend: 68,
  // Body and colour. The dark preset's inner light sits at 0.47 opacity,
  // which disappears against this surface; the brightness and saturation
  // are the lift the library's own large-host type carries.
  innerOpacity: 1.3,
  bloomOpacity: 1.12,
  brightness: 1.3,
  saturation: 1.45,
  // Pauses: a little more presence, breathing a little quicker.
  idle: 0.22,
  breatheDuration: 4,
  // How fast the colours travel under the voice, and the hue drift.
  flow: 58,
  hueDuration: 9,
} as const;

export function OverlayGlow({ mode, size }: { mode: GlowMode; size: OverlayPreferences["size"] }) {
  const radius = { s: 19, m: 23, l: 27 }[size];
  const rawId = useId().replace(/:/g, "-");
  const id = `sotto${rawId}`;
  const ref = useRef<HTMLDivElement>(null);
  const modeRef = useRef(mode);
  modeRef.current = mode;
  const levelRef = useRef(0);

  const css = useMemo(
    () =>
      `${generateVoiceBeamCSS({
        id,
        borderRadius: radius,
        borderWidth: BORDER_WIDTH,
        strokeOpacity: PRESET.strokeOpacity * DEFAULTS.strokeOpacity,
        innerOpacity: PRESET.innerOpacity * TUNING.innerOpacity,
        bloomOpacity: PRESET.bloomOpacity * TUNING.bloomOpacity,
        innerShadow: PRESET.innerShadow,
        colorVariant: "colorful",
        brightness: TUNING.brightness,
        saturation: TUNING.saturation,
        theme: THEME,
        hueBase: PRESET.hueBase ?? 0,
        glowSize: DEFAULTS.glowSize * SCALE,
        glowWidth: DEFAULTS.glowWidth * SCALE,
        glowHeight: DEFAULTS.glowHeight * SCALE,
        strokeScale: DEFAULTS.strokeScale,
        innerScale: DEFAULTS.innerScale,
        innerHeight: DEFAULTS.innerHeight,
        bloomScale: DEFAULTS.bloomScale,
        bloomHeight: DEFAULTS.bloomHeight,
        coreSize: DEFAULTS.coreSize * SCALE,
        coreLight: DEFAULTS.coreLight,
        coreLightWidth: DEFAULTS.coreLightWidth,
        coreLightHeight: DEFAULTS.coreLightHeight,
        rangeWidth: DEFAULTS.rangeWidth * SCALE,
        rangeHeight: DEFAULTS.rangeHeight * SCALE,
        softness: DEFAULTS.softness,
        distortion: DISTORTION_ON,
        scale: SCALE,
      })}
.overlay-beam[data-voice-beam="${id}"] {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
  z-index: 0;
}`,
    [id, radius],
  );

  const driverConfig = useMemo<VoiceDriverConfig>(
    () => ({
      id,
      // The level arrives already mapped to 0..1 by `display_level` in
      // audio.rs, and the driver applies `sensitivity` only on its own
      // Web Audio path, so this stays at unity.
      sensitivity: 1,
      threshold: 0.015,
      attack: TUNING.attack,
      release: TUNING.release,
      idle: TUNING.idle,
      breatheDuration: TUNING.breatheDuration,
      reach: TUNING.reach,
      spread: DEFAULTS.spread,
      bands: true,
      flow: TUNING.flow * SCALE,
      lobeSpacing: Math.max(0.1, DEFAULTS.lobeSpacing * SCALE),
      bend: TUNING.bend * SCALE,
      bandStrength: DEFAULTS.bandStrength,
      bandWidth: DEFAULTS.bandWidth * SCALE,
      bandPosition: DEFAULTS.bandPosition,
      bandCurve: DEFAULTS.bandCurve,
      bandSpread: DEFAULTS.bandSpread,
      bandSkew: DEFAULTS.bandSkew,
      bandOffset: DEFAULTS.bandOffset * SCALE,
      bandTail: DEFAULTS.bandTail,
      bandTailPosition: DEFAULTS.bandTailPosition,
      bandTailCurve: DEFAULTS.bandTailCurve,
      bandTailOverflow: DEFAULTS.bandTailOverflow * SCALE,
      bandAberration: DEFAULTS.bandAberration,
      rangeWidth: DEFAULTS.rangeWidth * SCALE,
      rangeHeight: DEFAULTS.rangeHeight * SCALE,
      theme: THEME,
      bandColors: BAND_COLORS,
      distortion: DISTORTION_ON ? DEFAULTS.distortion : 0,
      coreLight: DEFAULTS.coreLight,
      scale: SCALE,
      radius,
      processing: mode === "process",
      // Overlay pacing, deliberately not the geometry defaults: a pass here
      // is slower than the library's ~350px chat input (1.1 s) because the
      // card is wider, and the beam is held brighter so the wait reads as
      // work rather than a stall. `processingEase` has no preset value.
      processingDuration: 1.5,
      processingLevel: 0.72,
      processingEase: 0.75,
      processingTravel: 1,
      cornerFollow: 0,
      hueRange: PRESET.hueRange ?? 24,
      hueDuration: TUNING.hueDuration,
      staticColors: false,
      reducedMotion: typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches,
      paused: false,
    }),
    [id, mode, radius],
  );

  useEffect(() => {
    const stop = subscribe<{ level?: number }>("audio-level", (payload) => {
      const level = payload?.level;
      levelRef.current = typeof level === "number" ? level : 0;
    });
    return stop;
  }, []);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    return registerVoiceInstance(
      el,
      driverConfig,
      {
        getLevel: () => (modeRef.current === "listen" ? levelRef.current : 0),
      },
    );
  }, [driverConfig]);

  const filterId = `vb-distort-${id}`;
  // Packaged Tauri pages authorize dynamic styles with the bundled style nonce.
  const styleNonce = document.head.querySelector<HTMLStyleElement>("style[nonce]")?.nonce;
  return (
    <>
      <style nonce={styleNonce}>{css}</style>
      <div
        className="overlay-beam"
        ref={ref}
        data-voice-beam={id}
        data-voice-halfres=""
        data-active=""
        data-processing={mode === "process" ? "" : undefined}
        aria-hidden="true"
        style={{ "--voice-strength": TYPE_STYLE.strength ?? PRESET.strength ?? 1 } as CSSProperties}
      >
        <div data-voice-beam-bloom />
        {DISTORTION_ON && (
          <>
            <div data-voice-beam-warp="inner" />
            <div data-voice-beam-warp="bloom" />
          </>
        )}
        {!CANVAS_FILTER && <canvas data-voice-beam-band-halo aria-hidden="true" />}
        <canvas data-voice-beam-band aria-hidden="true" />
        {DISTORTION_ON && (
          <svg aria-hidden="true" width="0" height="0" style={{ position: "absolute", pointerEvents: "none" }}>
            <filter id={filterId} x="-20%" y="-20%" width="140%" height="140%" colorInterpolationFilters="sRGB">
              <feTurbulence type="fractalNoise" baseFrequency={`${(0.012 * DEFAULTS.distortionDetail).toFixed(4)} ${(0.05 * DEFAULTS.distortionDetail).toFixed(4)}`} numOctaves={2} seed={7} result="noise" />
              <feOffset in="noise" dx="0" dy="0" result="moved" />
              <feColorMatrix in="moved" type="matrix" values="1 0 0 0 0  0 0 0 0 0.5  0 0 0 0 0  0 0 0 0 1" result="map" />
              <feDisplacementMap in="SourceGraphic" in2="map" scale={0} xChannelSelector="R" yChannelSelector="G" />
            </filter>
          </svg>
        )}
      </div>
    </>
  );
}
