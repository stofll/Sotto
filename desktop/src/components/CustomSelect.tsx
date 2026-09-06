import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";
import { t } from "../i18n";
import { useAnchoredMenu } from "./anchoredMenu";
import { optionMatches } from "./selectSearch";

export type SelectOption<T extends string | number | null> = {
  value: T;
  label: string;
  meta?: string;
  icon?: string;
  disabled?: boolean;
};

// There is deliberately no `title` on the button or on the options: it repeated
// word for word what is written in them. It was there as a fallback for a label
// clipped to an ellipsis, but the menu is laid out to its content — the full
// text is one click away, and until then the bubble said «neura — neura».

/**
 * `alsoRef` is for a menu portalled into body: in the DOM it lies outside
 * `ref`, and without a second check a pointerdown on a list item would count as
 * a click outside and close the menu before the selection fired.
 */
export function useOutsideClose(
  open: boolean,
  ref: RefObject<HTMLElement | null>,
  onClose: () => void,
  alsoRef?: RefObject<HTMLElement | null>,
) {
  useEffect(() => {
    if (!open) return;

    function onPointerDown(event: PointerEvent) {
      const target = event.target as Node;
      if (ref.current?.contains(target)) return;
      if (alsoRef?.current?.contains(target)) return;
      onClose();
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open, ref, onClose, alsoRef]);
}

export function CustomSelect<T extends string | number | null>({ value, options, onChange, onOpen, className = "", disabled = false, searchable = false, inlineMeta = false, metaSeparator = "parens" }: { value: T; options: Array<SelectOption<T>>; onChange: (value: T) => void; onOpen?: () => void; className?: string; disabled?: boolean; searchable?: boolean; inlineMeta?: boolean; metaSeparator?: "parens" | "dash" }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const selected = options.find((option) => option.value === value || (option.value == null && value == null)) ?? options[0];
  const { menuRef, style: menuStyle, placed } = useAnchoredMenu(open, rootRef, 300);
  useOutsideClose(open, rootRef, () => setOpen(false), menuRef);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  // What was typed lives no longer than the open menu: coming back to the list
  // and finding it filtered by a previous search means concluding that half the
  // items have disappeared.
  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  // The list opens with the cursor already in the search box: nobody scans a
  // hundred languages by eye, and an extra click on the field turns search into
  // an optional afterthought.
  //
  // Not `autoFocus` and not simply "on open": on the first pass the menu hangs
  // in the DOM with `visibility: hidden` and a layout effect computes its
  // coordinates. Focus given at that moment goes nowhere — an invisible field
  // does not receive it, and no error is raised either. We wait until it is
  // painted in place.
  useEffect(() => {
    if (open && searchable && placed) searchRef.current?.focus();
  }, [open, searchable, placed]);

  const visible = useMemo(
    () => (searchable ? options.filter((option) => optionMatches(option, query)) : options),
    [options, query, searchable],
  );

  if (!selected) return null;

  // The menu is portalled into body, so a modifier put on the root would not
  // reach it: both need the class.
  const metaMod = inlineMeta && metaSeparator === "dash" ? "meta-dash" : "";
  // An option row reserves a column for the icon so that rows with and without
  // one line up. When no option in the list has an icon the reserved column is
  // zero wide — but the grid still draws its gutter, and every row in the menu
  // started 8px in from nothing. Read from `options` rather than the filtered
  // list, so a search that hides the only icon does not shift the rows.
  const menuHasIcons = options.some((option) => option.icon);

  const pick = (option: SelectOption<T>) => {
    if (disabled || option.disabled) return;
    setOpen(false);
    onChange(option.value);
  };

  return (
    <div className={`custom-select ${inlineMeta ? "custom-select--inline-meta " : ""}${metaMod ? `custom-select--${metaMod} ` : ""}${className}`} ref={rootRef}>
      <button className="custom-select__button" type="button" disabled={disabled} aria-haspopup="listbox" aria-expanded={open} onClick={() => { if (!open) onOpen?.(); setOpen((current) => !current); }}>
        {selected.icon && <Icon name={selected.icon} size={14}/>}
        <span className="custom-select__text">
          <span className="custom-select__label">{selected.label}</span>
          {selected.meta && <span className="custom-select__meta">{selected.meta}</span>}
        </span>
        <Icon name="chev-down" size={14} className="custom-select__chev"/>
      </button>
      {open && createPortal((
        <div className={`custom-select__menu${searchable ? " custom-select__menu--search" : ""}${inlineMeta ? " custom-select__menu--inline-meta" : ""}${metaMod ? ` custom-select__menu--${metaMod}` : ""}${menuHasIcons ? "" : " custom-select__menu--no-icons"}`} role="listbox" ref={menuRef} style={menuStyle}>
          {searchable && (
            <div className="custom-select__search">
              <Icon name="search" size={13}/>
              <input
                ref={searchRef}
                className="field"
                type="search"
                value={query}
                placeholder={t("Поиск")}
                aria-label={t("Поиск")}
                onChange={(event) => setQuery(event.target.value)}
                // Enter picks the first match: type «нем», press it, done —
                // without reaching for the mouse.
                onKeyDown={(event) => {
                  if (event.key !== "Enter") return;
                  event.preventDefault();
                  const first = visible.find((option) => !option.disabled);
                  if (first) pick(first);
                }}
              />
            </div>
          )}
          <div className={searchable ? "custom-select__list" : undefined}>
            {visible.map((option, index) => {
              const active = option.value === selected.value || (option.value == null && selected.value == null);
              return (
                <button key={`${option.value ?? "default"}-${index}`} className="custom-select__option" type="button" role="option" aria-selected={active} disabled={disabled || option.disabled} aria-disabled={disabled || option.disabled} onClick={() => pick(option)}>
                  {option.icon && <Icon name={option.icon} size={14}/>}
                  <span className="custom-select__text">
                    <span className="custom-select__label">{option.label}</span>
                    {option.meta && <span className="custom-select__meta">{option.meta}</span>}
                  </span>
                  {active && <span className="custom-select__check"><Icon name="check" size={14}/></span>}
                </button>
              );
            })}
            {visible.length === 0 && <p className="custom-select__empty">{t("Ничего не нашлось")}</p>}
          </div>
        </div>
      ), document.body)}
    </div>
  );
}
