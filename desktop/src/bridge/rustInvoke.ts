import { invoke as tauriInvoke } from "@tauri-apps/api/core";

/**
 * Direct Tauri command caller. The companion `bridge/invoke.ts` is a thin
 * wrapper over `tauriInvoke` — use `rustInvoke` for clarity in new code,
 * but both work.
 */
export async function rustInvoke<T>(method: string, params?: unknown): Promise<T> {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
        throw new Error(
            `rustInvoke('${method}') requires Tauri runtime. ` +
            `Run via 'pnpm tauri dev' or 'pnpm tauri build'.`
        );
    }
    return await tauriInvoke<T>(method, params as Record<string, unknown> | undefined);
}