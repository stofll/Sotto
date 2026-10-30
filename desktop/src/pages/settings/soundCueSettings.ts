import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type { ConfigResult, SoundCue, SoundCueSetting, SoundTheme } from "../../bridge/types";

// The defaults are duplicated in src-tauri/src/sounds.rs.
export const DEFAULT_SOUND_VOLUME = 0.35;
export const DEFAULT_SOUND_THEME: SoundTheme = "wood";

// The sliders move in decibels: on a linear amplitude scale everything between
// "barely there" and "quiet" sits in the first few pixels. The overall slider
// reaches lower, because "almost inaudible" is the setting people ask for.
export const OVERALL_RANGE_DB = 40;
export const CUE_RANGE_DB = 30;

/** Amplitude for a slider position 0..100, the bottom being `-rangeDb`. */
export function amplitudeAt(position: number, rangeDb: number): number {
  const clamped = Math.min(100, Math.max(0, position));
  return 10 ** (((clamped / 100 - 1) * rangeDb) / 20);
}

/** Slider position 0..100 for a saved amplitude. */
export function positionOf(amplitude: number, rangeDb: number): number {
  if (!(amplitude > 0)) return 0;
  const position = 100 + (100 * 20 * Math.log10(amplitude)) / rangeDb;
  return Math.min(100, Math.max(0, Math.round(position)));
}

/** A cue's settings with the defaults filled in. */
export function cueSetting(config: ConfigResult | null, cue: SoundCue): SoundCueSetting {
  const saved = config?.sound_cues?.[cue];
  return {
    source: saved?.source ?? "theme",
    theme: saved?.theme ?? null,
    volume: saved?.volume ?? 1,
    file_name: saved?.file_name ?? "",
  };
}

/** A config patch changing one cue's settings. */
export function cuePatch(cue: SoundCue, change: Partial<SoundCueSetting>): Partial<ConfigResult> {
  return { sound_cues: { [cue]: change } };
}

/** Play one cue as it would sound with these settings, before they are saved.
 *  `theme` is the overall one; the cue's own theme, if set, wins. */
export function previewCue(cue: SoundCue, theme: SoundTheme, setting: SoundCueSetting, overall: number) {
  if (setting.source === "mute") return;
  void tauriInvoke("preview_sound_cue", { cue, theme: setting.theme ?? theme, source: setting.source, volume: overall * setting.volume }).catch(() => {});
}
