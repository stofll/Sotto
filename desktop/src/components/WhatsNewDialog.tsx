import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { invoke, type ReleaseNotes } from "../bridge";
import { t } from "../i18n";
import { Modal } from "./Modal";

function PlainNotes({ text }: { text: string }) {
  return <div className="release-notes"><p>{text}</p></div>;
}

const ReleaseNotesContent = lazy(() => import("./ReleaseNotes").catch((error: unknown) => {
  console.error("Could not load release notes renderer:", error);
  return { default: PlainNotes };
}));

export function WhatsNewDialog({ ready }: { ready: boolean }) {
  const [release, setRelease] = useState<ReleaseNotes | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<"save" | "browser" | null>(null);
  const request = useRef<Promise<ReleaseNotes | null> | null>(null);
  useEffect(() => {
    let disposed = false;
    // Reuse the startup request when StrictMode replays effect setup.
    request.current ??= invoke<ReleaseNotes | null>("get_whats_new");
    void request.current.then((result) => {
      if (!disposed) setRelease(result);
    }).catch((error: unknown) => {
      if (!disposed) console.error("Could not load release notes:", error);
    });
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

  function openUrl(url: string) {
    setError(null);
    void invoke("open_url", { url }).catch(() => setError("browser"));
  }

  if (!release || !ready) return null;
  return <Modal title={t("Что нового")} onClose={() => void close()} busy={busy}>
    <div className="modal__body">
      <div className="release-notes__version">Sotto {release.version}</div>
      <Suspense fallback={<PlainNotes text={release.notes}/>}>
        <ReleaseNotesContent text={release.notes} onOpenUrl={openUrl}/>
      </Suspense>
      {error && <p role="alert">{error === "save"
        ? t("Не удалось сохранить отметку о просмотре. Попробуйте закрыть окно ещё раз.")
        : t("Не удалось открыть браузер. Попробуйте ещё раз.")}</p>}
    </div>
    <div className="modal__foot">
      <button className="btn btn--ghost" type="button" onClick={() => openUrl(release.url)}>{t("Релиз на GitHub")}</button>
      <button className="btn btn--primary" type="button" disabled={busy} onClick={() => void close()}>{t("Закрыть")}</button>
    </div>
  </Modal>;
}
