import { afterEach, expect, it, vi } from "vitest";
import { getStats, listHistory } from "./stats";

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

it("sends history filters and page bounds together and preserves backend counts", async () => {
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  const response = {
    entries: [{ id: 81, timestamp: 100, text: "Запись", length: 6 }],
    total_count: 120, filtered_count: 61, page: 1, page_size: 50,
    max_age_seconds: 2592000, max_entries: 1000, known_ids: [81],
  };
  invoke.mockResolvedValue(response);
  const query = { page: 1, page_size: 50, query: "ЗАПИСЬ_%", status: "processed" as const, since: 99, focus_id: 81, known_ids: [81, 82] };
  expect(await listHistory(query)).toEqual(response);
  expect(invoke).toHaveBeenCalledWith("list_history", { query });
});
