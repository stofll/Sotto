import { rustInvoke } from "./rustInvoke";
import type {
    StatsResult,
    HistoryAiPreview,
    HistoryEntry,
    HistoryListResult,
    HistoryRetryAiResult,
} from "./types";

// Re-export types from types.ts — single source of truth (do not duplicate).
// WS 4b Task 13.
export type {
    StatsResult,
    HistoryAiPreview,
    HistoryEntry,
    HistoryListResult,
    HistoryRetryAiResult,
};

/**
 * Stats + history bridge using rustInvoke (Tauri commands, no Python round-trip).
 *
 * Backend implementation lives in `desktop/src-tauri/src/{stats,history}.rs`
 * and `lib.rs` Tauri commands. The DB is a single SQLite file at
 * `~/.speech_to_text/sotto.db` — see `db.rs`.
 */

export async function getStats(): Promise<StatsResult> {
    return await rustInvoke<StatsResult>("get_stats");
}

export async function listHistory(): Promise<HistoryListResult> {
    return await rustInvoke<HistoryListResult>("list_history");
}

export async function deleteHistoryEntry(id: number): Promise<{ deleted: boolean }> {
    return await rustInvoke<{ deleted: boolean }>("delete_history_entry", { id });
}

export async function clearHistory(): Promise<{ deleted: number }> {
    return await rustInvoke<{ deleted: number }>("clear_history");
}

/**
 * Re-run the LLM over an existing entry WITHOUT storing anything: the caller
 * shows the result next to the current text and decides what to do with it.
 *
 * `profileId` picks one of the saved AI profiles; omit it to use the one
 * configured for dictation. `systemPrompt` is that profile's prompt already
 * resolved against its preset (`effectiveSystemPrompt`) — Rust stores presets
 * nowhere and expects finished text, exactly as on the dictation path.
 */
export async function previewHistoryAiProcessing(
    id: number,
    profileId?: string,
    systemPrompt?: string,
): Promise<HistoryAiPreview> {
    return await rustInvoke<HistoryAiPreview>("preview_history_ai_processing", {
        id,
        profileId: profileId ?? null,
        systemPrompt: systemPrompt ?? null,
    });
}

/**
 * Store a previewed result on its entry. `aiJson` / `statsJson` come from the
 * preview unchanged, so the row's badge and timings describe the run that was
 * accepted rather than an older attempt.
 *
 * Returns the refreshed entry so the caller can merge it into local state
 * without an extra `listHistory()` round-trip.
 */
export async function applyHistoryAiProcessing(
    id: number,
    text: string,
    aiJson: string,
    statsJson: string,
): Promise<HistoryRetryAiResult> {
    return await rustInvoke<HistoryRetryAiResult>("apply_history_ai_processing", {
        id,
        text,
        aiJson,
        statsJson,
    });
}