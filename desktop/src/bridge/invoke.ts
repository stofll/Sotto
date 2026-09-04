import { invoke as tauriInvoke } from "@tauri-apps/api/core";

let _tauriAvailable: boolean | null = null;

function hasTauri(): boolean {
  if (_tauriAvailable === null) {
    _tauriAvailable = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  }
  return _tauriAvailable;
}

export async function invoke<T>(method: string, params?: unknown): Promise<T> {
  if (!hasTauri()) {
    throw new Error(
      `invoke('${method}') requires Tauri runtime. ` +
      `Run via 'pnpm tauri dev' or 'pnpm tauri build'.`
    );
  }
  try {
    return await tauriInvoke<T>(method, params as any);
  } catch (e) {
    throw new Error(e instanceof Error ? e.message : String(e));
  }
}
