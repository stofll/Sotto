import { getCurrentWindow } from "@tauri-apps/api/window";

/** Whether the window is on screen, focused or not. Assumes it is when the native query fails. */
export async function isWindowShown(): Promise<boolean> {
  const windowHandle = getCurrentWindow();
  try {
    const [visible, minimized] = await Promise.all([windowHandle.isVisible(), windowHandle.isMinimized()]);
    return visible && !minimized;
  } catch {
    return true;
  }
}

/** Native focus/visibility is authoritative: WebView2 can stay "visible" in a hidden window. */
export function subscribeWindowActivity(onActive: (active: boolean) => void): () => void {
  const windowHandle = getCurrentWindow();
  let disposed = false;
  let generation = 0;
  let unlisten: (() => void) | undefined;

  function deactivate() {
    generation++;
    if (!disposed) onActive(false);
  }

  async function refresh() {
    const request = ++generation;
    if (document.visibilityState !== "visible") { onActive(false); return; }
    try {
      const [visible, minimized, focused] = await Promise.all([
        windowHandle.isVisible(), windowHandle.isMinimized(), windowHandle.isFocused(),
      ]);
      if (!disposed && request === generation) onActive(visible && !minimized && focused);
    } catch {
      if (!disposed && request === generation) onActive(false);
    }
  }

  void windowHandle.onFocusChanged(({ payload }) => {
    if (disposed) return;
    if (payload) void refresh();
    else deactivate();
  }).then((stop) => {
    if (disposed) stop();
    else unlisten = stop;
  }).catch(() => deactivate());
  const onFocus = () => { void refresh(); };
  window.addEventListener("focus", onFocus);
  window.addEventListener("blur", deactivate);
  document.addEventListener("visibilitychange", onFocus);
  void refresh();

  return () => {
    if (disposed) return;
    disposed = true;
    generation++;
    unlisten?.();
    window.removeEventListener("focus", onFocus);
    window.removeEventListener("blur", deactivate);
    document.removeEventListener("visibilitychange", onFocus);
  };
}
