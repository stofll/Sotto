import { lazy, Suspense } from "react";
import type { SceneProps } from "../../overlay/OverlayScene";

// The settings window shows thumbnails of the overlay long before anyone
// opens them (they sit in a folded section). The scene and its drawings load
// on first use, so they stay out of the settings window's startup budget.
// A chunk that fails to load leaves an empty thumbnail, as the scene's own drawings do.
const Scene = lazy(() => import("../../overlay/OverlayScene").then((m) => ({ default: m.OverlayScene }))
  .catch(() => ({ default: () => <></> })));

/** A still picture of the overlay drawn by the same scene as the real window. */
export function StillScene(props: SceneProps) {
  return <Suspense fallback={null}><Scene {...props}/></Suspense>;
}
