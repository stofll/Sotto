import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent as ReactPointerEvent, type RefObject } from "react";
import type { RecipeStyle } from "../../overlay/overlayRecipe";

type Radius = RecipeStyle["radius"];
/** From sharp to round: dragging a corner into the shell rounds it. */
const ORDER: Radius[] = ["sharp", "soft", "round"];
/** Screen pixels of drag, along a corner's diagonal, per step. */
const STEP = 14;
/** Each corner, and the direction that points into the shell from it. */
const CORNERS = [
  { key: "tl", x: 1, y: 1 }, { key: "tr", x: -1, y: 1 },
  { key: "bl", x: 1, y: -1 }, { key: "br", x: -1, y: -1 },
] as const;

/** The step a drag that started at `from` lands on after moving `depth` pixels into the shell. */
export const radiusAfterDrag = (from: Radius, depth: number): Radius =>
  ORDER[Math.max(0, Math.min(ORDER.length - 1, ORDER.indexOf(from) + Math.round(depth / STEP)))];

type Box = { left: number; top: number; right: number; bottom: number };

/**
 * Four corner brackets just outside the shell that set its corner style by
 * dragging. They snap to the three styles a recipe has, rather than a free
 * radius: the style also decides square matrix dots and square buttons,
 * which a number could not.
 */
export function RadiusHandle({ stageRef, value, label, names, remeasure, onPreview, onCommit }: {
  stageRef: RefObject<HTMLDivElement | null>; value: Radius; label: string; names: Record<Radius, string>;
  /** Anything that moves or resizes the shell without resizing it in the DOM, such as the stage's scale. */
  remeasure: unknown;
  onPreview: (radius: Radius | null) => void; onCommit: (radius: Radius) => void;
}) {
  const [box, setBox] = useState<Box | null>(null);
  const drag = useRef<{ x: number; y: number; dx: number; dy: number; last: Radius } | null>(null);

  // A passive effect: the stage's ref is attached after a child's layout effects run.
  useEffect(() => {
    const stage = stageRef.current;
    const skin = stage?.querySelector<HTMLElement>(".ove-compose .ovs-skin");
    const frame = stage?.querySelector<HTMLElement>(".ove-compose .ove-window");
    if (!stage || !skin) { setBox(null); return; }
    const measure = () => {
      const bounds = stage.getBoundingClientRect(), shell = skin.getBoundingClientRect();
      setBox({ left: shell.left - bounds.left, top: shell.top - bounds.top, right: shell.right - bounds.left, bottom: shell.bottom - bounds.top });
    };
    measure();
    // The shell animates between sizes; its window re-centres it as it does.
    const observer = new ResizeObserver(measure);
    observer.observe(skin);
    if (frame) observer.observe(frame);
    skin.addEventListener("transitionend", measure);
    return () => { observer.disconnect(); skin.removeEventListener("transitionend", measure); };
  }, [stageRef, remeasure]);

  if (!box) return null;
  const index = ORDER.indexOf(value);
  const step = (event: KeyboardEvent) => {
    const delta = ["ArrowRight", "ArrowUp"].includes(event.key) ? 1 : ["ArrowLeft", "ArrowDown"].includes(event.key) ? -1 : 0;
    if (!delta) return;
    event.preventDefault();
    const next = ORDER[Math.max(0, Math.min(ORDER.length - 1, index + delta))];
    if (next !== value) onCommit(next);
  };
  const move = (event: ReactPointerEvent) => {
    const current = drag.current;
    if (!current) return;
    const depth = ((event.clientX - current.x) * current.dx + (event.clientY - current.y) * current.dy) / 2;
    const next = radiusAfterDrag(value, depth);
    if (next !== current.last) { current.last = next; onPreview(next); }
  };
  const end = (commit: boolean) => {
    const last = drag.current?.last;
    drag.current = null;
    onPreview(null);
    if (commit && last && last !== value) onCommit(last);
  };
  // One slider for assistive technology; the other three corners repeat it for the pointer.
  return <div className="ove-radius" data-radius={value}>
    {CORNERS.map((corner, order) => <div key={corner.key} className={`ove-radius__corner ove-radius__corner--${corner.key}`}
      style={{ left: corner.x > 0 ? box.left : box.right, top: corner.y > 0 ? box.top : box.bottom }}
      {...(order === 0
        ? { role: "slider", tabIndex: 0, "aria-label": label, "aria-valuemin": 0, "aria-valuemax": ORDER.length - 1, "aria-valuenow": index, "aria-valuetext": names[value], onKeyDown: step }
        : { "aria-hidden": true })}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.stopPropagation();
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { x: event.clientX, y: event.clientY, dx: corner.x, dy: corner.y, last: value };
      }}
      onPointerMove={move} onPointerUp={() => end(true)} onPointerCancel={() => end(false)}/>)}
  </div>;
}
