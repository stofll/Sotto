import { useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from "react";

/**
 * Positioning a dropdown menu portalled into body.
 *
 * An absolute menu inside the page is clipped by the nearest scroller: both
 * `.win__main` and `.modal__body` have `overflow: auto`, and a list that did not
 * fit above the bottom edge was cut off — the remainder could not be scrolled
 * to, it simply was not drawn. Fixed coordinates computed from the button do not
 * know that boundary: the menu is clamped to the window.
 *
 * It always opens below. Flipping upward when room ran short made the same
 * menu open on a different side depending on how far the page was scrolled; a
 * list that does not fit below scrolls instead, and so does the page.
 *
 * Recalculation is hooked to scroll (in the capture phase — scrolling happens
 * in an inner container, not in the window) and to resize.
 */
const GAP = 6;
const MARGIN = 8;
const MIN_USABLE = 140;
const MIN_MENU_W = 180;

export type AnchoredMenu = {
  menuRef: RefObject<HTMLDivElement | null>;
  style: CSSProperties;
  /**
   * The menu is already positioned and visible.
   *
   * Before that it hangs in the DOM with `visibility: hidden`, and an invisible
   * element cannot be focused: `focus()` inside such a menu silently does
   * nothing. Whoever needs focus should wait for this flag.
   */
  placed: boolean;
};

/**
 * `start` — the menu's left edge against the button's left edge, `end` — right
 * against right.
 *
 * An action menu on a button in the top-right corner of a card has to open into
 * the card, not out of it: aligned to the left edge it runs past the card border
 * and hangs over the neighbour.
 */
export type MenuAlign = "start" | "end";

/**
 * How tall a menu of `natural` height may be with `below` pixels under the
 * button, and whether it has to scroll. A scrolling menu reserves room for its
 * scrollbar: otherwise the bar is drawn inside the width measured for the
 * content, and the items shrink and get cut short.
 */
export function menuPlacement(below: number, natural: number, maxHeight: number) {
  const height = Math.max(MIN_USABLE, Math.min(maxHeight, below));
  return { height, scrolls: natural > height };
}

export function useAnchoredMenu(
  open: boolean,
  anchorRef: RefObject<HTMLElement | null>,
  maxHeight = 260,
  align: MenuAlign = "start",
): AnchoredMenu {
  const menuRef = useRef<HTMLDivElement | null>(null);
  const [box, setBox] = useState<{ left: number; top: number; minWidth: number; maxHeight: number; scrolls: boolean } | null>(null);

  useLayoutEffect(() => {
    if (!open) {
      setBox(null);
      return;
    }
    const place = () => {
      const anchor = anchorRef.current?.getBoundingClientRect();
      if (!anchor) return;
      const below = window.innerHeight - anchor.bottom - GAP - MARGIN;
      const menu = menuRef.current;
      // The whole list, not the height it is clamped to right now: on a
      // re-placement during scroll `offsetHeight` is already capped.
      const natural = menu ? menu.scrollHeight + menu.offsetHeight - menu.clientHeight : maxHeight;
      const { height, scrolls } = menuPlacement(below, natural, maxHeight);
      // Width and height are taken from the already-rendered menu: they depend
      // on the content and the CSS, not on the button.
      const width = menuRef.current?.offsetWidth ?? anchor.width;
      const maxLeft = Math.max(MARGIN, window.innerWidth - width - MARGIN);
      const wanted = align === "end" ? anchor.right - width : anchor.left;
      setBox({
        left: Math.min(Math.max(MARGIN, wanted), maxLeft),
        top: anchor.bottom + GAP,
        // The menu is as wide as its own content (`width: max-content` in the
        // stylesheet), not as wide as the button. Stretching it to the button
        // left a field-wide strip of empty space beside three short model ids;
        // the floor only keeps a menu hung off a narrow button — an icon, a
        // «…» — from collapsing to the width of one word.
        minWidth: Math.min(anchor.width, MIN_MENU_W),
        maxHeight: height,
        scrolls,
      });
    };
    place();
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
    };
  }, [open, maxHeight, align, anchorRef]);

  return {
    menuRef,
    placed: box !== null,
    // First pass — the menu is in the DOM but not yet measured: we hide it so it
    // does not flash in the top-left corner. useLayoutEffect lands before paint.
    style: box
      ? { left: box.left, top: box.top, minWidth: box.minWidth, maxHeight: box.maxHeight, scrollbarGutter: box.scrolls ? "stable" : undefined }
      : { left: 0, top: 0, visibility: "hidden" },
  };
}
