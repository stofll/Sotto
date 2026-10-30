import { t } from "../i18n";

// The numbers behind the data elements of a recipe (limit, words, pace,
// engine, target), kept apart from how they are drawn.

/** Words per minute are noise for the first seconds of a recording. */
export const PACE_AFTER_SECONDS = 3;
/** The longest application name the overlay shows; the native side sends a display name, not a path. */
export const TARGET_MAX_CHARS = 40;

export const countWords = (text: string) => text.split(/\s+/).filter(Boolean).length;

/** Words per minute, or null while the recording is too young to say. */
export function wordsPerMinute(words: number, elapsedSeconds: number): number | null {
  if (!(elapsedSeconds > PACE_AFTER_SECONDS) || words <= 0) return null;
  return Math.round(words / (elapsedSeconds / 60));
}

/** How much of the recording limit has been used, 0..1, or null when there is no limit. */
export function limitProgress(elapsedSeconds: number, limitSeconds: number | null): number | null {
  if (!limitSeconds || limitSeconds <= 0) return null;
  return Math.max(0, Math.min(1, elapsedSeconds / limitSeconds));
}

/** The limit in seconds from `recording_limit_minutes` (already normalised), or null for none. */
export const limitSecondsOf = (minutes: number) => minutes > 0 ? minutes * 60 : null;

/** The cloud engine is the one the pipeline mode names; it is the rule `active_engine` follows natively. */
export const usesCloudEngine = (pipelineMode: string | undefined) => pipelineMode === "cloud";

/** The application name from the native side, made safe to draw: one line, no control characters, bounded. */
export function targetLabel(raw: unknown): string {
  if (typeof raw !== "string") return "";
  const line = raw.replace(/[\u0000-\u001f\u007f‎‏‪-‮]+/g, " ").replace(/\s+/g, " ").trim();
  return Array.from(line).slice(0, TARGET_MAX_CHARS).join("").trim();
}

/** What the scene needs to know that is not in the recording's own state. */
export type SceneFigures = {
  /** Seconds recorded so far. */
  elapsed: number;
  /** The recording limit in seconds, or null for none. */
  limitSeconds: number | null;
  cloud: boolean;
  /** The application the text will go to, or "" when the system gave no name. */
  target: string;
};

/** A believable set for a thumbnail, which has no recording behind it. */
export const sampleFigures = (): SceneFigures => ({ elapsed: 25, limitSeconds: 60, cloud: false, target: t("Заметки") });
