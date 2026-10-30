import { useEffect, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { invoke } from "../bridge/invoke";
import type { NotchFit } from "./overlayRecipe";

/** A reply that is a real cut-out: two finite, positive numbers. Anything else draws the named size. */
export function notchFit(raw: unknown): NotchFit | null {
  if (!raw || typeof raw !== "object") return null;
  const { width, height } = raw as Record<string, unknown>;
  return typeof width === "number" && typeof height === "number" && Number.isFinite(width) && Number.isFinite(height) && width > 0 && height > 0
    ? { width, height } : null;
}

/**
 * The camera notch the native side placed the notch shell around. Rust sends
 * it as "overlay-notch" each time it places the band, before the state that
 * shows it, so a monitor change never leaves the scene on an old cut-out. A
 * window created for this show missed that event; the last placement, asked
 * once, answers instead. Without a notch (or for any other shell) the answer
 * is null and the scene draws its own centre.
 */
export function useOverlayNotch(wanted: boolean) {
  const [fit, setFit] = useState<NotchFit | null>(null);
  useEffect(() => {
    setFit(null);
    if (!wanted) return;
    let disposed = false, heard = false;
    const unlisten = getCurrentWebviewWindow().listen<unknown>("overlay-notch", (event) => {
      if (disposed) return;
      heard = true;
      setFit(notchFit(event.payload));
    });
    // Skip StrictMode's discarded setup; a later event also outranks this reply.
    queueMicrotask(() => {
      if (disposed) return;
      void invoke<unknown>("overlay_notch").then((raw) => { if (!disposed && !heard) setFit(notchFit(raw)); }).catch(() => {});
    });
    return () => { disposed = true; void unlisten.then((stop) => stop()).catch(() => {}); };
  }, [wanted]);
  return fit;
}
