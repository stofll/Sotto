import { listen } from "@tauri-apps/api/event";

export type EventHandler<T> = (payload: T) => void;

type UnlistenFn = () => void;

function hasTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function on<T>(event: string, handler: EventHandler<T>): Promise<UnlistenFn> {
  if (!hasTauri()) {
    throw new Error(
      `events.on('${event}') requires Tauri runtime. ` +
      `Run via 'pnpm tauri dev' or 'pnpm tauri build'.`
    );
  }
  return await listen<T>(event, (e) => handler(e.payload));
}
