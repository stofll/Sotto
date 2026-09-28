/**
 * How many saved recordings to keep before the oldest is deleted.
 *
 * The values are duplicated in `src-tauri/src/recordings.rs` (`limit`): the
 * pruning there enforces what this list shows.
 */
import { minuteChoices } from "./minuteChoices";

export const DEFAULT_RECORDING_KEEP = 50;

/** `0` — no limit. */
const RECORDING_KEEP_CHOICES = [50, 200, 1000, 0];

/** What the pruning actually does for such a config value. */
export function recordingKeepCount(value: number | undefined | null): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 0) {
    return DEFAULT_RECORDING_KEEP;
  }
  return value;
}

export function recordingKeepOptions(current: number): number[] {
  return minuteChoices(RECORDING_KEEP_CHOICES, current);
}
