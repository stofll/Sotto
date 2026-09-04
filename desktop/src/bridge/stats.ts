import { rustInvoke } from "./rustInvoke";
import type {
    StatsResult,
    HistoryEntry,
    HistoryListResult,
    HistoryRetryAiResult,
    HistoryUpdateTextResult,
} from "./types";

// Re-export types from types.ts — single source of truth (do not duplicate).
// WS 4b Task 13.
export type {
    StatsResult,
    HistoryEntry,
    HistoryListResult,
    HistoryRetryAiResult,
    HistoryUpdateTextResult,
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

/**
 * Update entry text. Returns the refreshed entry so the caller can merge it
 * into local state without an extra `listHistory()` round-trip.
 */
export async function updateHistoryEntryText(id: number, text: string): Promise<HistoryUpdateTextResult> {
    return await rustInvoke<HistoryUpdateTextResult>("update_history_entry_text", { id, text });
}

export async function clearHistory(): Promise<{ deleted: number }> {
    return await rustInvoke<{ deleted: number }>("clear_history");
}

/**
 * Re-run AI processing on an existing entry. Rust reads the row, calls
 * Python's `compute_ai_for_retry` for the LLM step, and persists the result.
 *
 * Return shape `HistoryRetryAiResult { updated, entry?, reason? }` mirrors the
 * legacy Python handler so callers can do `result.updated && result.entry`
 * pattern matching.
 */
export async function retryHistoryAiProcessing(id: number): Promise<HistoryRetryAiResult> {
    return await rustInvoke<HistoryRetryAiResult>("retry_history_ai_processing", { id });
}