import { useEffect, useState } from "react";
import { invoke, type ReleaseNotes } from "../bridge";
import { t } from "../i18n";
import { Modal } from "./Modal";

/** Release text stays inert: no remote HTML, images or automatic link requests. */
function Notes({ text }: { text: string }) {
  return <div className="release-notes">{text.split(/\n\s*\n/).map((block, index) => {
    const lines = block.trim().split("\n");
    if (lines.every((line) => /^[-*] /.test(line))) {
      return <ul key={index}>{lines.map((line, i) => <li key={i}>{line.slice(2)}</li>)}</ul>;
    }
    return <div key={index}>{lines.map((line, i) => /^#{1,6} /.test(line)
      ? <h3 key={i}>{line.replace(/^#{1,6} /, "")}</h3>
      : <p key={i}>{line}</p>)}</div>;
  })}</div>;
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
