import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke, on } from "../bridge";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import type { ConfigResult, ModelInfo } from "../bridge/types";
import { t } from "../i18n";
import { loadThenPersistModel } from "./modelSelection";
import { downloadToastCopy, fallbackLanguage, type DownloadProgressEvent } from "./modelCatalog";

export type ModelOperationStatus = {
  kind: "loading" | "ok" | "error" | "info";
  text: string;
  detail?: string;
  progress?: number | null;
  closing?: boolean;
  /** The model id, if this operation can be interrupted. */
  cancelModel?: string;
};

type Params = {
  models: ModelInfo[];
  /** The id of the currently selected model — needed in order to roll back. */
  value: string;
  language?: string;
  onConfigChanged: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null>;
  onModelsChanged: (models: ModelInfo[]) => void;
  /** Close the menu before showing the modal; on the catalog page there is
   *  nothing to close. */
  onBeforeDialog?: () => void;
};

/**
 * Downloading, deleting and switching models — one set for the whole app.
 *
 * The dropdown in settings and the catalog page do the same things to models
 * while looking different. Copies of this logic drifting apart would mean that a
 * model downloaded from one place behaves unlike the same model downloaded from
 * another — so all of it lives here and the markup is left to the caller.
 */
export function useModelActions({ models, value, language, onConfigChanged, onModelsChanged, onBeforeDialog }: Params) {
  // Lists rather than a single id: several downloads may be running, and
  // finishing the first cleared the "busy" mark from all the rest — the second
  // one's card offered «Скачать» again in the middle of its own download.
  const [deleting, setDeleting] = useState<string[]>([]);
  const [downloading, setDownloading] = useState<string[]>([]);
  const [status, setStatus] = useState<ModelOperationStatus | null>(null);
  const [pendingDownload, setPendingDownload] = useState<ModelInfo | null>(null);
  const [pendingDelete, setPendingDelete] = useState<ModelInfo | null>(null);
  const [pendingSelect, setPendingSelect] = useState<ModelInfo | null>(null);
  const toastDismissTimer = useRef<number | null>(null);
  const toastRemoveTimer = useRef<number | null>(null);
  // Models whose cancellation has already been requested. Bytes sent before the
  // downloader noticed the flag are still in flight — and without this mark they
  // would bring the toast back to «Скачиваю…» with a cancel button on top of one
  // already pressed.
  const cancelRequested = useRef<Set<string>>(new Set());
  // The same list as in state but readable synchronously: state updates by the
  // next frame, while two quick clicks on one button happen within a single
  // frame.
  const inFlight = useRef<Set<string>>(new Set());

  function clearToastTimers() {
    if (toastDismissTimer.current != null) window.clearTimeout(toastDismissTimer.current);
    if (toastRemoveTimer.current != null) window.clearTimeout(toastRemoveTimer.current);
    toastDismissTimer.current = null;
    toastRemoveTimer.current = null;
  }

  function dismissStatus() {
    clearToastTimers();
    setStatus((current) => current ? { ...current, closing: true } : current);
    toastRemoveTimer.current = window.setTimeout(() => {
      setStatus(null);
      toastRemoveTimer.current = null;
    }, 220);
  }

  function showStatus(next: ModelOperationStatus, dismissAfterMs?: number) {
    clearToastTimers();
    setStatus({ ...next, closing: false });
    if (dismissAfterMs != null) {
      toastDismissTimer.current = window.setTimeout(dismissStatus, dismissAfterMs);
    }
  }

  useEffect(() => () => clearToastTimers(), []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    on<DownloadProgressEvent>("model-download-progress", (payload) => {
      const modelLabel = models.find((item) => item.id === payload?.model)?.label ?? payload?.model ?? t("Модель");
      const others = [...inFlight.current].filter((id) => id !== payload?.model).length;
      const copy = downloadToastCopy(payload, modelLabel, [...cancelRequested.current], others);
      if (!copy) return;
      clearToastTimers();
      setStatus({ kind: "loading", closing: false, ...copy });
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, [models]);

  // Esc closes the confirmation. `useOutsideClose` is no help here: by the time
  // the modal is shown the menu is already closed and its listener removed.
  useEffect(() => {
    if (!pendingDownload && !pendingDelete && !pendingSelect) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setPendingDownload(null);
      setPendingDelete(null);
      setPendingSelect(null);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [pendingDownload, pendingDelete, pendingSelect]);

  // For a monolingual model the language is pinned to its own by the same patch
  // that saves the model itself: a separate write would leave a window in which
  // the config points at an impossible pair.
  function persistWithLanguageRule(model: ModelInfo) {
    const next = fallbackLanguage(model, language);
    return (patch: Partial<ConfigResult>) => onConfigChanged(
      next ? { ...patch, language: next } : patch,
    );
  }

  /**
   * A click on a model.
   *
   * We ask before switching rather than after: changing the model unloads the
   * previous one from memory and fills memory with the new one — on a weak
   * machine that is seconds of waiting, and a user who mis-clicked would get
   * them for no reason at all.
   */
  function requestSelect(model: ModelInfo) {
    // Its own download is running — a click on the card stays silent: offering
    // to download what is already downloading means opening a dialog whose
    // button will do nothing.
    if (inFlight.current.has(model.id)) return;
    // A click on an already selected model changes nothing — there is nothing
    // to ask about.
    if (model.id === value) return;
    onBeforeDialog?.();
    // The engine cannot activate a model that is not downloaded, and a click on
    // one used to end in a red line saying "model tiny not downloaded". But
    // selecting a model is an intention to use it, not a mistake: we ask whether
    // to download it instead of reporting the impossible.
    if (!model.downloaded && !model.local) {
      setPendingDownload(model);
      return;
    }
    setPendingSelect(model);
  }

  async function selectModel(model: ModelInfo) {
    setPendingSelect(null);
    if (model.id === value) return;
    showStatus({ kind: "loading", text: t("Переключаю модель…") });
    try {
      await loadThenPersistModel(
        model.id,
        value,
        (modelId) => tauriInvoke("set_model", { model: modelId }),
        persistWithLanguageRule(model),
      );
      showStatus({ kind: "ok", text: t("Модель активна: {p0}", { p0: model.label }) }, 5000);
    } catch (e) {
      console.warn("model selection failed; keeping the previous engine:", e);
      showStatus({ kind: "error", text: t("Не удалось активировать модель: {p0}", { p0: e instanceof Error ? e.message : String(e) }) }, 9000);
    }
  }

  function requestDownload(model: ModelInfo) {
    onBeforeDialog?.();
    setPendingDownload(model);
  }

  function requestDelete(model: ModelInfo) {
    onBeforeDialog?.();
    setPendingDelete(model);
  }

  async function deleteModel(model: ModelInfo) {
    setPendingDelete(null);
    setDeleting((current) => (current.includes(model.id) ? current : [...current, model.id]));
    try {
      await invoke("delete_model", { model: model.id });
      const next = await invoke<ModelInfo[]>("list_models");
      onModelsChanged(next);
      showStatus({ kind: "ok", text: t("Модель удалена: {p0}", { p0: model.label }) }, 7000);
    } catch (error) {
      showStatus({ kind: "error", text: t("Не удалось удалить модель: {p0}", { p0: error instanceof Error ? error.message : String(error) }) }, 9000);
    } finally {
      setDeleting((current) => current.filter((id) => id !== model.id));
    }
  }

  /**
   * Cancelling a download.
   *
   * The toast is not closed: the command does not return instantly, and a
   * vanished toast would look like "it cancelled" while bytes are still in
   * flight. `startDownload` will show the outcome itself once it has an answer.
   */
  async function cancelDownload(modelId: string) {
    cancelRequested.current.add(modelId);
    setStatus((current) => current ? { ...current, text: t("Отменяю…"), cancelModel: undefined } : current);
    try {
      await tauriInvoke("cancel_model_download", { model: modelId });
    } catch (error) {
      console.warn("cancel_model_download failed:", error);
    }
  }

  async function startDownload(model: ModelInfo) {
    // A second download of the same model would write the same `*.part`; the
    // backend rejects it too, but explaining to the user an error we allowed
    // ourselves is a poor way of not allowing it.
    if (inFlight.current.has(model.id)) return;
    inFlight.current.add(model.id);
    cancelRequested.current.delete(model.id);
    setDownloading((current) => (current.includes(model.id) ? current : [...current, model.id]));
    showStatus({ kind: "loading", text: t("Скачиваю {p0}", { p0: model.label }), detail: model.size, progress: null, cancelModel: model.id });
    try {
      // `null` means cancelled: the user got exactly what they asked for, and
      // that is not an error.
      const outcome = await tauriInvoke<unknown>("download_model", { model: model.id });
      if (outcome == null) {
        showStatus({ kind: "info", text: t("Загрузка отменена"), detail: model.label }, 5000);
        return;
      }
      const next = await invoke<ModelInfo[]>("list_models");
      onModelsChanged(next);
      // Downloaded means they wanted to use it: we switch to it right away.
      try {
        await loadThenPersistModel(
          model.id,
          value,
          (modelId) => tauriInvoke("set_model", { model: modelId }),
          persistWithLanguageRule(model),
        );
        showStatus({ kind: "ok", text: t("Модель скачана и активна: {p0}", { p0: model.label }), progress: 100 }, 7000);
      } catch (e) {
        console.warn("auto-load after download failed:", e);
        showStatus({ kind: "error", text: t("Модель скачана, но не удалось активировать: {p0}", { p0: e instanceof Error ? e.message : String(e) }) }, 9000);
      }
    } catch (e) {
      console.error("download_model failed:", e);
      showStatus({ kind: "error", text: t("Не удалось скачать модель: {p0}", { p0: e instanceof Error ? e.message : String(e) }) }, 9000);
    } finally {
      inFlight.current.delete(model.id);
      cancelRequested.current.delete(model.id);
      setDownloading((current) => current.filter((id) => id !== model.id));
    }
  }

  return {
    status,
    dismissStatus,
    deleting,
    downloading,
    /** Whether this model's card is busy with an operation of its own. */
    isBusy: (id: string) => downloading.includes(id) || deleting.includes(id),
    pendingDownload,
    pendingDelete,
    setPendingDownload,
    setPendingDelete,
    selectModel,
    requestSelect,
    pendingSelect,
    setPendingSelect,
    cancelDownload,
    requestDownload,
    requestDelete,
    deleteModel,
    startDownload,
  };
}

export type ModelActions = ReturnType<typeof useModelActions>;

/**
 * The progress toast and both confirmations. Rendered through a portal, so the
 * caller does not care where in the markup it places this.
 */
export function ModelActionOverlays({ actions }: { actions: ModelActions }) {
  const { status, dismissStatus, cancelDownload, pendingDownload, pendingDelete, pendingSelect, setPendingDownload, setPendingDelete, setPendingSelect, startDownload, deleteModel, selectModel } = actions;
  return (
    <>
      {status && createPortal((
        <div
          className={`model-download-toast${status.kind === "loading" ? " model-download-toast--progress" : ""}${status.closing ? " model-download-toast--closing" : ""}`}
          role={status.kind === "error" ? "alert" : "status"}
          aria-live="polite"
        >
          <span className={`model-download-toast__icon model-download-toast__icon--${status.kind}`}>
            {status.kind === "loading"
              ? <Icon name="download" size={16}/>
              : <Icon name={status.kind === "ok" ? "check" : "x"} size={15}/>
            }
          </span>
          <span className="model-download-toast__copy">
            <strong>{status.text}</strong>
            {status.detail && <span>{status.detail}</span>}
          </span>
          {/* While a download is running the close button cancels the download
              itself rather than hiding the toast. Both used to stand side by
              side: an «Отменить» button and a close button meaning "let it
              download, just out of my sight". Nobody looked for the second, and
              two similar actions in one corner cost more than they saved. The
              close button's label changes along with its action so cancellation
              is never pressed blindly. */}
          <button
            className="model-download-toast__close"
            type="button"
            onClick={() => { if (status.cancelModel) void cancelDownload(status.cancelModel); else dismissStatus(); }}
            aria-label={status.cancelModel ? t("Отменить скачивание") : t("Закрыть")}
            title={status.cancelModel ? t("Отменить скачивание") : t("Закрыть")}
          >
            <Icon name="x" size={13}/>
          </button>
          {status.kind === "loading" && (
            <span className="model-download-toast__track">
              <span
                className={status.progress == null ? "model-download-toast__bar model-download-toast__bar--indeterminate" : "model-download-toast__bar"}
                style={status.progress == null ? undefined : { width: `${status.progress}%` }}
              />
            </span>
          )}
        </div>
      ), document.body)}
      {pendingDownload && createPortal((
        <div className="modal-overlay" onMouseDown={(event) => { if (event.target === event.currentTarget) setPendingDownload(null); }}>
          <div className="modal" role="dialog" aria-modal="true" aria-label={t("Скачать модель?")} style={{ width: "min(420px, 100%)" }}>
            <div className="modal__head">
              <h2>{t("Скачать модель?")}</h2>
              <button className="modal__close" type="button" onClick={() => setPendingDownload(null)} aria-label={t("Закрыть")}><Icon name="x" size={14}/></button>
            </div>
            <div className="modal__body">
              <p style={{ margin: 0, font: "400 13px/1.5 var(--font-sans)", color: "var(--ink-mute)" }}>
                {t("Модель «{p0}» ещё не скачана — это {p1}. После загрузки она включится автоматически.", { p0: pendingDownload.label, p1: pendingDownload.size })}
              </p>
            </div>
            <div className="modal__foot">
              <button
                className="btn btn--primary"
                type="button"
                autoFocus
                onClick={() => { const model = pendingDownload; setPendingDownload(null); void startDownload(model); }}
              >
                <Icon name="download" size={12}/>{t("Скачать")}
              </button>
              <button className="btn btn--ghost" type="button" onClick={() => setPendingDownload(null)}>{t("Отмена")}</button>
            </div>
          </div>
        </div>
      ), document.body)}
      {pendingSelect && createPortal((
        <div className="modal-overlay" onMouseDown={(event) => { if (event.target === event.currentTarget) setPendingSelect(null); }}>
          {/* A close button is redundant here: «Отмена» next to it does the
              same thing, and two closing buttons in a dialog asking one
              question is a choice without a difference. */}
          <div className="modal" role="dialog" aria-modal="true" aria-label={t("Переключить модель?")} style={{ width: "min(320px, 100%)" }}>
            <div className="modal__head">
              <h2>{t("Переключить модель?")}</h2>
            </div>
            <div className="modal__foot">
              <button className="btn btn--primary" type="button" autoFocus onClick={() => void selectModel(pendingSelect)}>
                <Icon name="check" size={12}/>{t("Выбрать")}
              </button>
              <button className="btn btn--ghost" type="button" onClick={() => setPendingSelect(null)}>{t("Отмена")}</button>
            </div>
          </div>
        </div>
      ), document.body)}
      {pendingDelete && createPortal((
        <div className="modal-overlay" onMouseDown={(event) => { if (event.target === event.currentTarget) setPendingDelete(null); }}>
          <div className="modal" role="dialog" aria-modal="true" aria-label={t("Удалить модель?")} style={{ width: "min(420px, 100%)" }}>
            <div className="modal__head">
              <h2>{t("Удалить модель?")}</h2>
              <button className="modal__close" type="button" onClick={() => setPendingDelete(null)} aria-label={t("Закрыть")}><Icon name="x" size={14}/></button>
            </div>
            <div className="modal__body">
              {/* A user's own file and a downloaded one are deleted the same
                  way, but the consequences differ: a catalog model the app will
                  bring back itself, a foreign file never. That conversation
                  happens here, before the deletion. */}
              <p style={{ margin: 0, font: "400 13px/1.5 var(--font-sans)", color: pendingDelete.local ? "var(--warn)" : "var(--ink-mute)" }}>
                {pendingDelete.local
                  ? t("Файл «{p0}» ({p1}) будет удалён с диска навсегда. Это ваш файл: скачать его заново приложение не сможет.", { p0: pendingDelete.label, p1: pendingDelete.size })
                  : t("Файл модели «{p0}» ({p1}) будет удалён с диска. Для повторного использования его придётся скачать заново.", { p0: pendingDelete.label, p1: pendingDelete.size })}
              </p>
              {pendingDelete.loaded && (
                <p style={{ margin: 0, font: "400 13px/1.5 var(--font-sans)", color: "var(--warn)" }}>
                  {t("Эта модель сейчас загружена. Она будет выгружена из памяти, и распознавание перестанет работать, пока вы не выберете или не скачаете другую модель.")}
                </p>
              )}
            </div>
            <div className="modal__foot">
              <button className="btn btn--primary" type="button" autoFocus onClick={() => void deleteModel(pendingDelete)}>
                <Icon name="trash" size={12}/>{t("Удалить")}
              </button>
              <button className="btn btn--ghost" type="button" onClick={() => setPendingDelete(null)}>{t("Отмена")}</button>
            </div>
          </div>
        </div>
      ), document.body)}
    </>
  );
}
