import { useEffect, useRef, type ReactNode, type Ref } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";
import { t } from "../i18n";

/** A modal dialog: portalled, dismissed by Escape or a click on the overlay,
 * with focus trapped inside it and returned to the opener on close.
 *
 * Lifted out of `DictionaryLibrary`, where it was written, when the parasite
 * word list needed the same shell. `busy` blocks every exit while a save is in
 * flight, so a half-written change cannot be dismissed out from under itself.
 *
 * `showHeader={false}` is for a dialog whose whole content is one question:
 * the question is the body, and a header repeating it above the cross would
 * say it twice. `title` still names the dialog for assistive technology.
 * `closeRef` lets the opener aim focus at the cross when the dialog has no
 * cancel button of its own. `focusFirstInput={false}` keeps focus on the
 * dialog itself, for one that opens on a choice rather than on typing.
 */
export function Modal({ title, children, onClose, busy = false, className, showHeader = true, closeRef, focusFirstInput = true }: { title: string; children: ReactNode; onClose: () => void; busy?: boolean; className?: string; showHeader?: boolean; closeRef?: Ref<HTMLButtonElement>; focusFirstInput?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    (focusFirstInput ? ref.current?.querySelector<HTMLElement>("input") ?? ref.current : ref.current)?.focus();
    return () => { previous?.focus(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- focus is placed once, when the dialog opens.
  }, []);
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      // An open dropdown takes Escape for itself: this listener runs in the
      // capture phase, ahead of the menu's own, and used to close the whole
      // dialog — or ask about it — instead of the list.
      if (event.key === "Escape" && document.querySelector(".custom-select__menu")) return;
      if (event.key === "Escape" && !busy) { event.preventDefault(); event.stopPropagation(); onClose(); }
      if (event.key !== "Tab") return;
      const dialog = ref.current;
      if (!dialog) return;
      const items = Array.from(dialog.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), [tabindex="0"]')).filter((element) => !element.closest('fieldset:disabled'));
      const first = items[0]; const last = items[items.length - 1];
      if (!first) { event.preventDefault(); dialog.focus(); return; }
      if (!dialog.contains(document.activeElement) || document.activeElement === dialog) {
        event.preventDefault(); (event.shiftKey ? last : first).focus();
      } else if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    }
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [busy, onClose]);
  return createPortal(<div className="modal-overlay" onMouseDown={(event) => { if (!busy && event.target === event.currentTarget) onClose(); }}>
    <div className={className ? `modal ${className}` : "modal"} ref={ref} tabIndex={-1} role="dialog" aria-modal="true" aria-label={title}>
      {showHeader && <div className="modal__head"><h2>{title}</h2><button ref={closeRef} type="button" className="modal__close" disabled={busy} onClick={onClose} aria-label={t("Закрыть")}><Icon name="x" size={14}/></button></div>}
      {children}
    </div>
  </div>, document.body);
}
