import { useEffect, useState, type ReactNode } from "react";
import { invoke, type ReleaseNotes } from "../bridge";
import { t } from "../i18n";
import { Modal } from "./Modal";

/** The lines of one blank-line-separated block, as headings, text and lists.
 *
 * A run of bullets is collected wherever it appears rather than only when the
 * whole block is one: GitHub's own generated notes put the list straight under
 * its heading, and treating that block as prose printed the leading "- " as
 * text. Blocks stay the unit of spacing, so `.release-notes > * + *` still
 * separates sections while the lines inside one keep their tight rhythm. */
function blockNodes(block: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  let bullets: string[] = [];
  const flushBullets = () => {
    if (bullets.length === 0) return;
    nodes.push(<ul key={nodes.length}>{bullets.map((item, i) => <li key={i}>{item}</li>)}</ul>);
    bullets = [];
  };
  for (const line of block.split("\n")) {
    if (/^[-*] /.test(line)) { bullets.push(line.slice(2)); continue; }
    flushBullets();
    if (line) nodes.push(/^#{1,6} /.test(line)
      ? <h3 key={nodes.length}>{line.replace(/^#{1,6} /, "")}</h3>
      : <p key={nodes.length}>{line}</p>);
  }
  flushBullets();
  return nodes;
}

/** Release text stays inert: no remote HTML, images or automatic link requests. */
function Notes({ text }: { text: string }) {
  // GitHub sends CRLF; a stray carriage return would otherwise reach the DOM.
  return <div className="release-notes">{text.replace(/\r\n?/g, "\n").split(/\n\s*\n/)
    .map((block, index) => <div key={index}>{blockNodes(block.trim())}</div>)}</div>;
}

export function WhatsNewDialog({ ready }: { ready: boolean }) {
  const [release, setRelease] = useState<ReleaseNotes | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<"save" | "browser" | null>(null);
  useEffect(() => {
    let disposed = false;
    void invoke<ReleaseNotes | null>("get_whats_new").then((result) => {
      if (!disposed) setRelease(result);
    }).catch(() => { /* Optional notes must not interrupt startup when offline. */ });
    return () => { disposed = true; };
  }, []);

  async function close() {
    if (!release || busy) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("dismiss_whats_new", { version: release.version });
      setRelease(null);
    } catch { setError("save"); }
    finally { setBusy(false); }
  }

  if (!release || !ready) return null;
  return <Modal title={t("Что нового")} onClose={() => void close()} busy={busy}>
    <div className="modal__body">
      <div className="release-notes__version">Sotto {release.version}</div>
      <Notes text={release.notes}/>
      {error && <p role="alert">{error === "save"
        ? t("Не удалось сохранить отметку о просмотре. Попробуйте закрыть окно ещё раз.")
        : t("Не удалось открыть браузер. Попробуйте ещё раз.")}</p>}
    </div>
    <div className="modal__foot">
      <button className="btn btn--ghost" type="button" onClick={() => {
        void invoke("open_url", { url: release.url }).catch(() => setError("browser"));
      }}>{t("Релиз на GitHub")}</button>
      <button className="btn btn--primary" type="button" disabled={busy} onClick={() => void close()}>{t("Закрыть")}</button>
    </div>
  </Modal>;
}
