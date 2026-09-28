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

export function WhatsNewDialog({ ready, onClose }: { ready: boolean; onClose?: () => void }) {
  const manual = !!onClose;
  const [release, setRelease] = useState<ReleaseNotes | null>(null);
  const [loading, setLoading] = useState(true);
  const [attempt, setAttempt] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<"save" | "browser" | null>(null);
  const request = useRef<Promise<ReleaseNotes | null> | null>(null);
  useEffect(() => {
    let disposed = false;
    // Reuse each request when StrictMode replays effect setup.
    request.current ??= invoke<ReleaseNotes | null>("get_whats_new", { manual });
    void request.current.then((result) => {
      if (!disposed) setRelease(result);
    }).catch((error: unknown) => {
      if (!disposed) console.error("Could not load release notes:", error);
    }).finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, [manual, attempt]);

  function retry() {
    request.current = null;
    setLoading(true);
    setAttempt((value) => value + 1);
  }

  async function close() {
    if (busy) return;
    if (manual) { onClose(); return; }
    if (!release) return;
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

  if (!ready || (!manual && !release)) return null;
  return <Modal title={t("Что нового")} onClose={() => void close()} busy={busy}>
    <div className="modal__body">
      {release ? <>
        <div className="release-notes__version">Sotto {release.version}</div>
        <Suspense fallback={<PlainNotes text={release.notes}/>}>
          <ReleaseNotesContent text={release.notes} onOpenUrl={openUrl}/>
        </Suspense>
      </> : <p role="status">{loading ? t("Загружаем список изменений…") : t("Список изменений пока недоступен. Проверьте подключение и попробуйте ещё раз.")}</p>}
      {error && <p role="alert">{error === "save"
        ? t("Не удалось сохранить отметку о просмотре. Попробуйте закрыть окно ещё раз.")
        : t("Не удалось открыть браузер. Попробуйте ещё раз.")}</p>}
    </div>
    <div className="modal__foot">
      {release && <button className="btn btn--ghost" type="button" onClick={() => openUrl(release.url)}>{t("Релиз на GitHub")}</button>}
      {!release && !loading && <button className="btn btn--ghost" type="button" onClick={retry}>{t("Повторить")}</button>}
      <button className="btn btn--primary" type="button" disabled={busy} onClick={() => void close()}>{t("Закрыть")}</button>
    </div>
  </Modal>;
}
