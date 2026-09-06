import { useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Icon } from "./Icon";
import { t } from "../i18n";

/**
 * A yes/no question about something that cannot be undone, asked in the app's
 * own dialog rather than the OS one.
 *
 * Not `window.confirm`: in this app's WebView2 it returns `true` without ever
 * drawing anything, so every question asked through it was answered «да» by
 * nobody. The dialog plugin drew a real window, but a grey system box with an
 * OK button belongs to no application in particular; this is the same `.modal`
 * the model download and deletion questions use.
 *
 * The promise resolves `false` for every way out that is not the confirm
 * button — Escape, the close button, a click outside, so a question that
 * somehow fails to be answered destroys nothing.
 */
export function confirmDestructive(message: string, confirmLabel = t("Удалить")): Promise<boolean> {
  return new Promise((resolve) => {
    const previous = document.activeElement as HTMLElement | null;
    const answer = (ok: boolean) => {
      render(null);
      // The dialog took focus away from the button that opened it; without
      // this the page is left with nothing focused and Tab starts over.
      previous?.focus?.();
      resolve(ok);
    };
    render(<ConfirmDialog message={message} confirmLabel={confirmLabel} onAnswer={answer}/>);
  });
}

let host: HTMLDivElement | null = null;
let root: Root | null = null;

/** One host element and one React root for every question the app ever asks:
 *  a fresh `createRoot` per call leaks a container into the body each time. */
function render(dialog: React.ReactNode) {
  if (!host) {
    host = document.createElement("div");
    document.body.appendChild(host);
  }
  root ??= createRoot(host);
  root.render(dialog);
}

function ConfirmDialog({ message, confirmLabel, onAnswer }: {
  message: string;
  confirmLabel: string;
  onAnswer: (ok: boolean) => void;
}) {
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") onAnswer(false);
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onAnswer]);

  return (
    <div className="modal-overlay" onMouseDown={(event) => { if (event.target === event.currentTarget) onAnswer(false); }}>
      <div className="modal modal--ask" role="alertdialog" aria-modal="true" aria-label={message}>
        <div className="modal__head">
          <h2>{t("Подтвердите действие")}</h2>
          <button className="modal__close" type="button" onClick={() => onAnswer(false)} aria-label={t("Закрыть")}>
            <Icon name="x" size={14}/>
          </button>
        </div>
        <div className="modal__body">
          <p style={{ margin: 0, font: "400 13px/1.5 var(--font-sans)", color: "var(--ink-mute)" }}>{message}</p>
        </div>
        <div className="modal__foot">
          <button className="btn btn--primary" type="button" autoFocus onClick={() => onAnswer(true)}>
            <Icon name="trash" size={12}/>{confirmLabel}
          </button>
          <button className="btn btn--ghost" type="button" onClick={() => onAnswer(false)}>{t("Отмена")}</button>
        </div>
      </div>
    </div>
  );
}
