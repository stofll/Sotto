import { subscribe } from "../bridge/events";

/** Where a level drawing takes its readings from: the microphone in the
 *  overlay, a simulated voice in the settings previews. Readings are 0..1 and
 *  arrive about every 33 ms. */
export type LevelSource = (listener: (level: number) => void) => () => void;

export function clampLevel(level: unknown) {
  return typeof level === "number" && Number.isFinite(level) ? Math.max(0, Math.min(1, level)) : 0;
}

export const audioLevelSource: LevelSource = (listener) =>
  subscribe<{ level?: number }>("audio-level", (payload) => listener(clampLevel(payload?.level)));
