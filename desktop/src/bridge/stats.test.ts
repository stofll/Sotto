import { afterEach, expect, it, vi } from "vitest";
import { getStats } from "./stats";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
afterEach(() => { vi.unstubAllGlobals(); invoke.mockReset(); });

it("preserves speech timing coverage and excluded silence in the statistics response", async () => {
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  invoke.mockResolvedValue({
    total_transcriptions: 3,
    total_audio_seconds: 55,
    total_excluded_silence_seconds: 20,
    total_speech_timed_transcriptions: 2,
    daily_history: [{ date: "2026-09-22", count: 3, audio_seconds: 55, excluded_silence_seconds: 20, speech_timed_count: 2 }],
  });
  const stats = await getStats();
  expect(stats.total_audio_seconds - (stats.total_excluded_silence_seconds ?? 0)).toBe(35);
  expect(stats.total_transcriptions - (stats.total_speech_timed_transcriptions ?? 0)).toBe(1);
  expect(stats.daily_history[0].excluded_silence_seconds).toBe(20);
  expect(stats.daily_history[0].speech_timed_count).toBe(2);
});
