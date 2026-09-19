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
 * cancel button of its own.
 */
export function Modal({ title, children, onClose, busy = false, className, showHeader = true, closeRef }: { title: string; children: ReactNode; onClose: () => void; busy?: boolean; className?: string; showHeader?: boolean; closeRef?: Ref<HTMLButtonElement> }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    (ref.current?.querySelector<HTMLElement>("input") ?? ref.current)?.focus();
    return () => { previous?.focus(); };
  }, []);
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
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
