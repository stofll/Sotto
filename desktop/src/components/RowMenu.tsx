import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";
import { Hint } from "./Hint";
import { useAnchoredMenu } from "./anchoredMenu";
import { useOutsideClose } from "./CustomSelect";

export type RowMenuItem = {
  id: string;
  label: string;
  icon: string;
  danger?: boolean;
  disabled?: boolean;
  onSelect: () => void;
};

/**
 * The secondary actions of a list row, behind a single «…».
 *
 * There used to be two patterns on «Провайдеры и ключи»: a profile revealed
 * three icons on hover, a key showed two of them permanently. Hover is
 * unreachable from a keyboard and from a touchpad tap, and a permanent row of
 * icons competes for attention with the row's own primary action. One button
 * per row solves both, and the actions get names instead of pictograms.
 */
export function RowMenu({ items, label }: { items: RowMenuItem[]; label: string }) {
  const [open, setOpen] = useState(false);
  const anchorRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const { menuRef, style, placed } = useAnchoredMenu(open, anchorRef, 220, "end");
  useOutsideClose(open, anchorRef, () => setOpen(false), menuRef);

  /** Closed from the keyboard: the «…» is where the focus came from and the
   *  only thing left on the row to put it back on. A mouse close leaves focus
   *  alone — nothing was taken from it. */
  function closeToTrigger() {
    setOpen(false);
    triggerRef.current?.focus();
  }

  // The menu is portalled to the end of `body`, so Tab from the button walks
  // on down the row instead of into it: the items are reachable only if the
  // focus is put there. Waiting for `placed` because until then the menu hangs
  // in the DOM with `visibility: hidden`, and `focus()` on it does nothing at
  // all — silently, as everywhere else in this codebase's menus.
  useEffect(() => {
    if (!open || !placed) return;
    menuRef.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [open, placed, menuRef]);

  return (
    // The row around it may itself be a button or a click target: the menu's
    // own clicks must not reach it.
    <div
      ref={anchorRef}
      className="row-menu"
      onClick={(event) => event.stopPropagation()}
      onKeyDown={(event) => { if (open && event.key === "Escape") { event.stopPropagation(); closeToTrigger(); } }}
    >
      <Hint text={label}>
        <button
          ref={triggerRef}
          type="button"
          className="icon-btn"
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={label}
          onClick={() => setOpen((current) => !current)}
        >
          <Icon name="more" size={14}/>
        </button>
      </Hint>
      {open && createPortal((
        <div
          className="custom-select__menu card-menu row-menu__list"
          role="menu"
          // Named after the button it belongs to: a bare `role="menu"` is
          // announced as «menu», which says nothing about whose it is, and the
          // portal puts it out of earshot of the row.
          aria-label={label}
          ref={menuRef}
          style={style}
          onKeyDown={(event) => { if (event.key === "Escape") { event.stopPropagation(); closeToTrigger(); } }}
        >
          {items.map((item) => (
            <button
              key={item.id}
              type="button"
              role="menuitem"
              className={item.danger ? "custom-select__option card-menu__item--danger" : "custom-select__option"}
              disabled={item.disabled}
              onClick={() => { closeToTrigger(); item.onSelect(); }}
            >
              <Icon name={item.icon} size={13}/>
              <span className="custom-select__text"><span className="custom-select__label">{item.label}</span></span>
            </button>
          ))}
        </div>
      ), document.body)}
    </div>
  );
}
