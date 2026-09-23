import { minuteChoices } from "./minuteChoices";

/**
 * How long the model stays idle before leaving RAM.
 *
 * The values are duplicated in `src-tauri/src/config.rs`: both sides read this
 * setting and they must not diverge — the UI would show one thing while the
 * engine unloaded by another.
 */

/** No value in the config — unload after five minutes, not "never". */
export const DEFAULT_MODEL_UNLOAD_MINUTES = 5;

/** `0` — do not unload. */
export const MODEL_UNLOAD_CHOICES = [5, 10, 30, 0];

/** A day is already "never", just written as a number. */
const MAX_MODEL_UNLOAD_MINUTES = 24 * 60;

/**
 * What the engine actually does for such a config value.
 *
 * Garbage and negative numbers fall back to the default rather than disabling
 * unloading: "we could not read it" is not "you asked for never". The order is
 * the same as in `config::model_unload_after_minutes`.
 */
export function modelUnloadMinutes(value: number | undefined | null): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 0) {
    return DEFAULT_MODEL_UNLOAD_MINUTES;
  }
  return Math.min(value, MAX_MODEL_UNLOAD_MINUTES);
}

/** Values for the settings dropdown; see {@link minuteChoices}. */
export function modelUnloadOptions(current: number): number[] {
  return minuteChoices(MODEL_UNLOAD_CHOICES, current);
}
