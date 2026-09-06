import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";
import { useOutsideClose } from "./CustomSelect";
import { useAnchoredMenu } from "./anchoredMenu";
import { t } from "../i18n";

/** From this many options the list gets a search box of its own. Below it, the
 *  list is short enough to read, and a search field would be one control more
 *  than the task needs. */
const SEARCH_FROM = 10;

/**
 * A model id: typed by hand, or picked from what the provider offers.
 *
 * Not a `<datalist>`. That is what the field on «Интеграции» used to be, and
 * the browser opens its list on its own terms — in WebView2 an unfocused click
 * usually opened nothing at all, so the suggestions were there and unreachable.
 * A menu we draw ourselves is built from the same `custom-select__*` parts as
 * every other dropdown in the app — same paddings, same row height, same tick
 * on the right. Only the trigger differs, and it has to: a model id can be
 * typed as well as picked.
 *
 * Because the trigger is a text field, the value and the query are the same
 * string, and that broke down on the providers that matter: OpenRouter returns
 * over four hundred models, and searching them meant first wiping the id the
 * field was holding. So above `SEARCH_FROM` options the query moves into the
 * menu — its own box, focused when the list is opened by the chevron, with
 * Enter on the first match, exactly like the language picker. Exactly one
 * control filters at a time: the search box when it exists, the field itself
 * when it does not.
 */
export function ModelCombobox({ value, suggestions, onChange, onCommit, placeholder, openSignal }: {
  value: string;
  suggestions: string[];
  onChange: (next: string) => void;
  /// The committed value is handed over rather than left to be read back from
  /// `value`: picking from the list changes and commits in one event, and by
  /// then the state behind `value` is one render behind — a commit that read it
  /// would write the id the field held before the click.
  onCommit: (value: string) => void;
  placeholder?: string;
  /// A counter: every change opens the list. This is how the «request the
  /// models» button shows what it has just fetched — otherwise the field
  /// reports «20 options from the provider» and keeps them to itself.
  openSignal?: number;
}) {
  const [open, setOpen] = useState(false);
  // The field almost always already holds a model id, and it is not a search
  // query — it is the answer. Filtering by it left the list with the one item
  // that was already chosen. We filter by it only once something is typed.
  const [typed, setTyped] = useState(false);
  const [query, setQuery] = useState("");
  // Whether this opening should put the cursor in the search box. Opening by
  // focusing the field must not: the cursor is needed where it is, in the id.
  const [searchWanted, setSearchWanted] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const { menuRef, style: menuStyle, placed } = useAnchoredMenu(open, rootRef, 300);
  useOutsideClose(open, rootRef, () => { setOpen(false); onCommit(value); }, menuRef);

  const hasSearch = suggestions.length > SEARCH_FROM;

  useEffect(() => {
    if (open) return;
    setQuery("");
    setSearchWanted(false);
  }, [open]);

  // An invisible field cannot take focus, and no error is raised either: we
  // wait until the menu is measured and painted in place.
  useEffect(() => {
    if (open && hasSearch && searchWanted && placed) searchRef.current?.focus();
  }, [open, hasSearch, searchWanted, placed]);

  useEffect(() => {
    if (openSignal === undefined || openSignal === 0) return;
    setTyped(false);
    setSearchWanted(true);
    setOpen(true);
  }, [openSignal]);

  const filtered = useMemo(() => {
    const raw = hasSearch ? query : (typed ? value : "");
    const q = raw.trim().toLowerCase();
    if (!q) return suggestions;
    return suggestions.filter((s) => s.toLowerCase().includes(q));
  }, [suggestions, hasSearch, query, typed, value]);

  function pick(next: string) {
    setTyped(false);
    onChange(next);
    setOpen(false);
    onCommit(next);
  }

  const menu = (body: ReactNode) => createPortal(
    <div
      className={`custom-select__menu${hasSearch ? " custom-select__menu--search" : ""}`}
      role="listbox"
      ref={menuRef}
      style={menuStyle}
    >
      {hasSearch && (
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
            onKeyDown={(event) => {
              if (event.key === "Escape") { setOpen(false); return; }
              if (event.key !== "Enter") return;
              // Enter picks the first match: type «gemma», press it, done.
              event.preventDefault();
              if (filtered[0]) pick(filtered[0]);
            }}
          />
        </div>
      )}
      <div className={hasSearch ? "custom-select__list" : undefined}>{body}</div>
    </div>,
    document.body,
  );

  return (
    <div className="combobox" ref={rootRef}>
      <input
        className="field mono"
        value={value}
        onChange={(e) => { setTyped(true); onChange(e.target.value); if (!open) setOpen(true); }}
        onFocus={() => setOpen(true)}
        onBlur={(event) => {
          // Focus moved into the menu — into the search box or onto an option.
          // The field has not been left, the widget is still in use.
          if (menuRef.current?.contains(event.relatedTarget as Node | null)) return;
          setOpen(false);
          onCommit(value);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") { setOpen(false); onCommit(value); }
          if (e.key === "Escape") setOpen(false);
        }}
        placeholder={placeholder}
        style={{ width: "100%", height: 34 }}
      />
      <button
        type="button"
        className="icon-btn combobox__toggle"
        aria-label={t("Показать список моделей")}
        aria-expanded={open}
        // Mousedown would blur the field first and close what this click opens.
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          setTyped(false);
          setSearchWanted(true);
          setOpen((current) => !current);
        }}
      >
        <Icon name="chev-down" size={13}/>
      </button>
      {open && menu(filtered.length > 0
        ? filtered.map((s) => (
          <button
            key={s}
            type="button"
            role="option"
            className="custom-select__option"
            aria-selected={s === value}
            // The click must land before the input's blur closes the menu.
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => pick(s)}
          >
            <span className="custom-select__text">
              <span className="custom-select__label mono">{s}</span>
            </span>
            {s === value && <span className="custom-select__check"><Icon name="check" size={14}/></span>}
          </button>
        ))
        : <p className="custom-select__empty">{query.trim() ? t("Ничего не нашлось") : t("Нет известных моделей — введите id вручную.")}</p>
      )}
    </div>
  );
}
