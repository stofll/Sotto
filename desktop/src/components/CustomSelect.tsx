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

function optionTitle<T extends string | number | null>(option?: SelectOption<T>) {
  if (!option) return undefined;
  return option.meta ? `${option.label} - ${option.meta}` : option.label;
}

/**
 * `alsoRef` — для меню, вынесенного порталом в body: в DOM оно лежит вне
 * `ref`, и без второй проверки pointerdown по пункту списка считался бы
 * кликом снаружи и закрывал меню раньше, чем срабатывал выбор.
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

export function CustomSelect<T extends string | number | null>({ value, options, onChange, onOpen, className = "", disabled = false, searchable = false, inlineMeta = false }: { value: T; options: Array<SelectOption<T>>; onChange: (value: T) => void; onOpen?: () => void; className?: string; disabled?: boolean; searchable?: boolean; inlineMeta?: boolean }) {
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

  // Набранное живёт не дольше открытого меню: вернуться к списку и увидеть
  // его отфильтрованным прошлым поиском — значит решить, что половина
  // пунктов пропала.
  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  // Открыли список — курсор сразу в поиске: искать глазами по сотне языков
  // никто не станет, а лишний клик по полю превращает поиск в необязательный
  // довесок.
  //
  // Не `autoFocus` и не просто «на открытие»: первым проходом меню висит в
  // DOM с `visibility: hidden`, координаты ему считает layout-эффект. Фокус,
  // выданный в этот момент, уходит в никуда — невидимому полю его не дают, и
  // ошибки при этом не будет. Ждём отрисовки на месте.
  useEffect(() => {
    if (open && searchable && placed) searchRef.current?.focus();
  }, [open, searchable, placed]);

  const visible = useMemo(
    () => (searchable ? options.filter((option) => optionMatches(option, query)) : options),
    [options, query, searchable],
  );

  if (!selected) return null;

  const pick = (option: SelectOption<T>) => {
    if (disabled || option.disabled) return;
    setOpen(false);
    onChange(option.value);
  };

  return (
    <div className={`custom-select ${inlineMeta ? "custom-select--inline-meta " : ""}${className}`} ref={rootRef}>
      <button className="custom-select__button" type="button" disabled={disabled} aria-haspopup="listbox" aria-expanded={open} title={optionTitle(selected)} onClick={() => { if (!open) onOpen?.(); setOpen((current) => !current); }}>
        {selected.icon && <Icon name={selected.icon} size={14}/>}
        <span className="custom-select__text">
          <span className="custom-select__label">{selected.label}</span>
          {selected.meta && <span className="custom-select__meta">{selected.meta}</span>}
        </span>
        <Icon name="chev-down" size={14} className="custom-select__chev"/>
      </button>
      {open && createPortal((
        <div className={`custom-select__menu${searchable ? " custom-select__menu--search" : ""}${inlineMeta ? " custom-select__menu--inline-meta" : ""}`} role="listbox" ref={menuRef} style={menuStyle}>
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
                // Enter выбирает первый подходящий: набрал «нем», нажал —
                // готово, без перехода к мыши.
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
                <button key={`${option.value ?? "default"}-${index}`} className="custom-select__option" type="button" role="option" aria-selected={active} disabled={disabled || option.disabled} aria-disabled={disabled || option.disabled} title={optionTitle(option)} onClick={() => pick(option)}>
                  {option.icon && <Icon name={option.icon} size={14}/>}
                  <span className="custom-select__text">
                    <span className="custom-select__label">{option.label}</span>
                    {option.meta && <span className="custom-select__meta">{option.meta}</span>}
                  </span>
                  {active && <Icon name="check" size={14}/>}
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
