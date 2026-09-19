import { useEffect, useId, useMemo, useRef, type CSSProperties } from "react";
import { subscribe } from "../bridge/events";
import { themePresets, generateVoiceBeamCSS } from "./voice-glow/styles";
import { resolveVoiceDefaults, resolveVoiceStyle } from "./voice-glow/presets";
import { registerVoiceInstance, type VoiceDriverConfig } from "./voice-glow/voiceDriver";

export type GlowMode = "listen" | "process" | "idle";

const BAND_COLORS = { core: "255, 255, 255", above: "255, 70, 80", mid: "90, 255, 150", below: "80, 140, 255" } as const;
const THEME = "dark" as const;
const DEFAULTS = resolveVoiceDefaults("default", THEME);
const PRESET = themePresets[THEME];
const TYPE_STYLE = resolveVoiceStyle("default", THEME);
const SCALE = DEFAULTS.scale;
const RADIUS = 24;
const BORDER_WIDTH = 1;
const DISTORTION_ON = DEFAULTS.distortion > 0;
const CANVAS_FILTER = (() => {
  if (typeof document === "undefined") return true;
  const ctx = document.createElement("canvas").getContext("2d");
  return !!ctx && typeof (ctx as { filter?: unknown }).filter === "string";
})();

export function OverlayGlow({ mode }: { mode: GlowMode }) {
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
        borderRadius: RADIUS,
        borderWidth: BORDER_WIDTH,
        strokeOpacity: PRESET.strokeOpacity * DEFAULTS.strokeOpacity,
        innerOpacity: PRESET.innerOpacity * DEFAULTS.innerOpacity,
        bloomOpacity: PRESET.bloomOpacity * DEFAULTS.bloomOpacity,
        innerShadow: PRESET.innerShadow,
        colorVariant: "colorful",
        brightness: TYPE_STYLE.brightness ?? PRESET.brightness,
        saturation: TYPE_STYLE.saturation ?? PRESET.saturation,
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
    [id],
  );

  const driverConfig = useMemo<VoiceDriverConfig>(
    () => ({
      id,
      sensitivity: 3.1,
      threshold: 0.015,
      attack: 0.325,
      release: 0.86,
      idle: DEFAULTS.idle,
      breatheDuration: 5.2,
      reach: DEFAULTS.reach,
      spread: DEFAULTS.spread,
      bands: true,
      flow: DEFAULTS.flow * SCALE,
      lobeSpacing: Math.max(0.1, DEFAULTS.lobeSpacing * SCALE),
      bend: DEFAULTS.bend * SCALE,
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
      radius: RADIUS,
      processing: mode === "process",
      // Overlay pacing, deliberately not the geometry defaults: a pass here is
      // slower than the library's ~350px chat input (1.1 s), and the morph in
      // and out of the sweep is longer. `processingEase` has no preset value.
      processingDuration: 2.8,
      processingLevel: DEFAULTS.processingLevel,
      processingEase: 1.1,
      processingTravel: DEFAULTS.processingTravel,
      cornerFollow: DEFAULTS.cornerFollow,
      hueRange: PRESET.hueRange ?? 24,
      hueDuration: PRESET.hueDuration ?? 12,
      staticColors: false,
      reducedMotion: typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches,
      paused: false,
    }),
    [id, mode],
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
  return (
    <>
      <style>{css}</style>
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
