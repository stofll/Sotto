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

/** Own a component subscription even when registration finishes after cleanup. */
export function subscribe<T>(event: string, handler: EventHandler<T>): UnlistenFn {
  let disposed = false;
  let unlisten: UnlistenFn | undefined;
  void on<T>(event, (payload) => {
    if (!disposed) handler(payload);
  }).then((stop) => {
    if (disposed) stop();
    else unlisten = stop;
  }).catch((error) => console.warn(`Could not subscribe to ${event}`, error));
  return () => {
    if (disposed) return;
    disposed = true;
    unlisten?.();
  };
}
