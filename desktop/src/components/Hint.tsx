import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";

const GAP = 8;      // gap between the icon and the bubble
const MARGIN = 8;   // minimum distance from the bubble to the window edge

/**
 * A hint with a bubble.
 *
 * Without `children` this is the familiar "i" icon. With `children` the control
 * itself becomes the anchor: people ask about it by hovering over it, not over
 * an icon next to it — so the explanation of why a device toggle is greyed out
 * lives on the toggle itself and takes no rows in the layout.
 *
 * The bubble is portalled into body with position: fixed and coordinates
 * computed from the icon: an absolute bubble inside the page was clipped by the
 * .main-body scroller as soon as the hint landed near a card edge. The position
 * is clamped to the window horizontally and flips downward when there is not
 * enough room above.
 */
// Neither size nor offset is a prop: every call site used to set them itself
// (13/18, 11/14, 10/13, 12/16), and the distance from label to icon matched
// nowhere. The geometry is single and lives in CSS — the reference is the
// «Горячая клавиша» row in settings.
// `className` and `style` go on the anchor. Wrapping a button turns the anchor
// into the flex or grid item in its place, so whatever held that place —
// `flex: 1`, `margin-left: auto`, a full-width cell — has to move onto it.
export function Hint({ text, children, className, style }: { text: string; children?: ReactNode; className?: string; style?: CSSProperties }) {
  const anchorRef = useRef<HTMLSpanElement>(null);
  const bubbleRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  useLayoutEffect(() => {
    if (!open) {
      setPos(null);
      return;
    }
    const anchor = anchorRef.current?.getBoundingClientRect();
    const bubble = bubbleRef.current?.getBoundingClientRect();
    if (!anchor || !bubble) return;
    const maxLeft = Math.max(MARGIN, window.innerWidth - bubble.width - MARGIN);
    const left = Math.min(Math.max(MARGIN, anchor.left + anchor.width / 2 - bubble.width / 2), maxLeft);
    const above = anchor.top - GAP - bubble.height;
    const top = above >= MARGIN ? above : anchor.bottom + GAP;
    setPos({ left, top });
  }, [open, text]);

  // The bubble is fixed relative to the window and does not follow scrolling —
  // closing it is simpler than recomputing on every scroll frame.
  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
    };
  }, [open]);

  return (
    <span
      ref={anchorRef}
      className={`${children ? "hint-anchor" : "hint"}${className ? ` ${className}` : ""}`}
      style={style}
      // The icon does not take focus on its own — it is given focus, otherwise
      // the hint exists for the mouse only. A wrapper around a control needs no
      // focus of its own and is harmed by it: it becomes an extra stop before
      // the control itself.
      tabIndex={children ? undefined : 0}
      // A name on the wrapper would replace the name of the control inside, so
      // the text goes to the screen reader as a separate line rather than as a
      // label on the anchor.
      aria-label={children ? undefined : text}
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
      onFocus={() => setOpen(true)}
      onBlur={() => setOpen(false)}
    >
      {children ?? <Icon name="info" size={11}/>}
      {children && <span className="sr-only">{text}</span>}
      {open && createPortal((
        <div
          ref={bubbleRef}
          className="hint-bubble"
          role="tooltip"
          style={pos ? { left: pos.left, top: pos.top } : { left: 0, top: 0, visibility: "hidden" }}
        >
          {text}
        </div>
      ), document.body)}
    </span>
  );
}
