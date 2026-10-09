/**
 * Stands in for desktop/src/bridge/events.ts when the site builds the app's
 * overlay: there is no Tauri runtime here, and every level the page shows comes
 * from a simulated voice passed in explicitly. See astro.config.mjs.
 */
export type EventHandler<T> = (payload: T) => void;

export function subscribe<T>(_event: string, _handler: EventHandler<T>): () => void {
  return () => {};
}
