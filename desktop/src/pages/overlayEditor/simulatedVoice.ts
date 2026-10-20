import { useEffect, useState } from "react";
import { t } from "../../i18n";
import type { LevelSource } from "../../overlay/levelSource";
import type { PastedExit, ScenePhase } from "../../overlay/overlayRecipe";

// A voice for the constructor: bursts of syllables and pauses at the rate the
// real `audio-level` event arrives. Every preview on the page shares one
// ticker, which runs only while something listens.

const TICK_MS = 33;
const listeners = new Set<(level: number) => void>();
let timer = 0;
const burst = { on: false, left: 0, amplitude: 0.6, phase: 0, rate: 0.6 };

function nextLevel() {
  if (burst.left <= 0) {
    burst.on = !burst.on;
    burst.left = burst.on ? 14 + Math.random() * 30 : 5 + Math.random() * 13;
    burst.amplitude = 0.35 + Math.random() * 0.65;
    burst.rate = 0.5 + Math.random() * 0.35;
  }
  burst.left--;
  if (!burst.on) return 0.012 + Math.random() * 0.03;
  burst.phase += burst.rate;
  return Math.min(1, burst.amplitude * Math.pow(Math.abs(Math.sin(burst.phase)), 0.7) * (0.72 + Math.random() * 0.38));
}

export const simulatedVoice: LevelSource = (listener) => {
  listeners.add(listener);
  if (!timer) timer = window.setInterval(() => { const level = nextLevel(); listeners.forEach((each) => each(level)); }, TICK_MS);
  return () => {
    listeners.delete(listener);
    if (!listeners.size) { window.clearInterval(timer); timer = 0; }
  };
};

/** A fixed stretch of a phrase, delivered at once: the still pictures of templates. */
export const stillVoice: LevelSource = (listener) => {
  for (let index = 0; index < 96; index++) {
    listener(Math.min(1, 0.55 * Math.pow(Math.abs(Math.sin(index * 0.62)), 0.7) * (0.7 + 0.3 * Math.sin(index * 0.21)) + 0.03));
  }
  return () => {};
};

/** A silent source: after recording the drawings rest. */
export const silentVoice: LevelSource = () => () => {};

export const PHASE_MODES = ["scenario", "recording", "streaming", "processing", "pasted", "error", "limit"] as const;
export type PhaseMode = typeof PHASE_MODES[number];

const WORD_MS = 255;
const mmss = (seconds: number) => `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
export const samplePhrase = () => t("Коллеги, по итогам созвона: релиз переносим на четверг, Маша обновляет сборку, а я до вечера пришлю список правок и новые скриншоты для магазина приложений.");

/** `leaving` is the overlay on its way out after the note, which is when the dissolve exit plays. */
export type SimulatedFrame = { phase: ScenePhase; shown: boolean; streaming: boolean; draft: string; timer: string; limited: boolean; leaving?: boolean; elapsed: number };

/** The limit the stage pretends is set, short enough that the ring is seen closing. */
export const SIMULATED_LIMIT_SECONDS = 60;

/** How long the pasted preview rests before it leaves, when the exit is worth watching. */
const PASTED_REST_MS = 1800;
const PASTED_LOOP_MS = 3000;

/** What the stage shows for the chosen phase, advanced four times a second. With a dissolve exit the pasted phase also plays its leaving. */
export function frameAt(mode: PhaseMode, elapsed: number, exit: PastedExit = "fade"): SimulatedFrame {
  const words = samplePhrase().split(" ");
  const spoken = (ms: number) => words.slice(0, Math.max(0, Math.min(words.length, Math.floor(ms / WORD_MS)))).join(" ");
  const clock = (ms: number) => mmss(Math.floor(ms / 1000) % 3600);
  switch (mode) {
    case "scenario": {
      const at = elapsed % 13000;
      if (at < 500) return { phase: "recording", shown: false, streaming: true, draft: "", timer: "00:00", limited: false, elapsed: 0 };
      if (at < 8000) return { phase: "recording", shown: true, streaming: true, draft: spoken(at - 1700), timer: clock(at - 500), limited: false, elapsed: Math.floor((at - 500) / 1000) };
      if (at < 10000) return { phase: "processing", shown: true, streaming: true, draft: words.join(" "), timer: "00:07", limited: false, elapsed: 7 };
      if (at < 11800) return { phase: "pasted", shown: true, streaming: true, draft: words.join(" "), timer: "00:07", limited: false, elapsed: 7 };
      return { phase: "pasted", shown: false, streaming: true, draft: "", timer: "00:07", limited: false, leaving: true, elapsed: 7 };
    }
    case "streaming": return { phase: "recording", shown: true, streaming: true, draft: spoken((elapsed % 10500) - 900), timer: clock(elapsed), limited: false, elapsed: Math.floor((elapsed % 10500) / 1000) };
    case "processing": return { phase: "processing", shown: true, streaming: true, draft: words.join(" "), timer: "00:07", limited: false, elapsed: 7 };
    case "pasted": {
      const leaving = exit === "dissolve" && elapsed % PASTED_LOOP_MS >= PASTED_REST_MS;
      return { phase: "pasted", shown: !leaving, streaming: true, draft: words.join(" "), timer: "00:07", limited: false, leaving, elapsed: 7 };
    }
    case "error": return { phase: "error", shown: true, streaming: false, draft: "", timer: "00:07", limited: false, elapsed: 7 };
    case "limit": {
      const remaining = Math.max(0, 59 - Math.floor(elapsed / 1000) % 60);
      return { phase: "recording", shown: true, streaming: false, draft: "", timer: mmss(remaining), limited: true, elapsed: SIMULATED_LIMIT_SECONDS - remaining };
    }
    default: return { phase: "recording", shown: true, streaming: false, draft: "", timer: clock(elapsed), limited: false, elapsed: Math.floor(elapsed / 1000) % SIMULATED_LIMIT_SECONDS };
  }
}

/** Re-render the stage every `interval` ms while `active`. */
export function useTicker(active: boolean, interval = 250) {
  const [now, setNow] = useState(() => performance.now());
  useEffect(() => {
    if (!active) return;
    // A ticker that was paused starts from the present, not from its last tick.
    setNow(performance.now());
    const id = window.setInterval(() => setNow(performance.now()), interval);
    return () => window.clearInterval(id);
  }, [active, interval]);
  return now;
}
