/**
 * After how many minutes a recording stops by itself and is transcribed.
 *
 * The values are duplicated in `src-tauri/src/config.rs`
 * (`recording_limit_minutes`): the recorder enforces what this list shows.
 */
import { minuteChoices } from "./minuteChoices";

export const DEFAULT_RECORDING_LIMIT_MINUTES = 15;

/** `0` — no limit. */
const RECORDING_LIMIT_CHOICES = [5, 10, 15, 30, 60, 0];

const MAX_RECORDING_LIMIT_MINUTES = 24 * 60;

/** What the recorder actually does for such a config value. */
export function recordingLimitMinutes(value: number | undefined | null): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 0) {
    return DEFAULT_RECORDING_LIMIT_MINUTES;
  }
  return Math.min(value, MAX_RECORDING_LIMIT_MINUTES);
}

export function recordingLimitOptions(current: number): number[] {
  return minuteChoices(RECORDING_LIMIT_CHOICES, current);
}
