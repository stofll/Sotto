import { cloneElement, isValidElement, useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type HTMLAttributes, type ReactNode, type Ref } from "react";
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
export function Hint({ text, children, className, style, asChild = false }: { text?: string; children?: ReactNode; className?: string; style?: CSSProperties; asChild?: boolean }) {
  const anchorRef = useRef<HTMLElement>(null);
  const bubbleId = useId();
  const bubbleRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  useLayoutEffect(() => {
    if (!open || !text) {
      setPos(null);
      return;
    }
    const anchor = anchorRef.current?.getBoundingClientRect();
    const bubble = bubbleRef.current?.getBoundingClientRect();
    if (!anchor || !bubble) return;
    const maxLeft = Math.max(MARGIN, window.innerWidth - bubble.width - MARGIN);
    const left = Math.min(Math.max(MARGIN, anchor.left + anchor.width / 2 - bubble.width / 2), maxLeft);
    const above = anchor.top - GAP - bubble.height;
    const preferredTop = above >= MARGIN ? above : anchor.bottom + GAP;
    const top = Math.max(MARGIN, Math.min(preferredTop, window.innerHeight - bubble.height - MARGIN));
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

  const bubble = open && text && createPortal(
    <div id={bubbleId} ref={bubbleRef} className="hint-bubble" role="tooltip"
      style={pos ? { left: pos.left, top: pos.top } : { left: 0, top: 0, visibility: "hidden" }}>
      {text}
    </div>, document.body);

  // Attach to the existing element so grid/flex placement and text truncation
  // do not change when replacing a native title tooltip.
  if (asChild && isValidElement<HTMLAttributes<HTMLElement> & { ref?: Ref<HTMLElement> }>(children)) {
    const props = children.props;
    return <>{cloneElement(children, {
      ref: (node: HTMLElement | null) => {
        anchorRef.current = node;
        if (typeof props.ref === "function") return props.ref(node);
        if (props.ref) props.ref.current = node;
      },
      "aria-describedby": open && text ? [props["aria-describedby"], bubbleId].filter(Boolean).join(" ") : props["aria-describedby"],
      onMouseEnter: (event) => { props.onMouseEnter?.(event); setOpen(true); },
      onMouseLeave: (event) => { props.onMouseLeave?.(event); setOpen(false); },
      onFocus: (event) => { props.onFocus?.(event); setOpen(true); },
      onBlur: (event) => { props.onBlur?.(event); setOpen(false); },
      onKeyDown: (event) => { props.onKeyDown?.(event); if (event.key === "Escape") setOpen(false); },
    })}{bubble}</>;
  }

  return <span
    ref={(node) => { anchorRef.current = node; }}
    className={`${children ? "hint-anchor" : "hint"}${className ? ` ${className}` : ""}`}
    style={style}
    tabIndex={children ? undefined : 0}
    aria-label={children ? undefined : text}
    aria-describedby={open && text ? bubbleId : undefined}
    onMouseEnter={() => setOpen(true)} onMouseLeave={() => setOpen(false)}
    onFocus={() => setOpen(true)} onBlur={() => setOpen(false)}
    onKeyDown={(event) => { if (event.key === "Escape") setOpen(false); }}
  >
    {children ?? <Icon name="info" size={11}/>}
    {children && <span className="sr-only">{text}</span>}
    {bubble}
  </span>;
}
