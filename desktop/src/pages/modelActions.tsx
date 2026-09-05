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
  /** Идентификатор модели, если эту операцию можно прервать. */
  cancelModel?: string;
};

type Params = {
  models: ModelInfo[];
  /** Идентификатор выбранной сейчас модели — нужен, чтобы откатиться. */
  value: string;
  language?: string;
  onConfigChanged: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null>;
  onModelsChanged: (models: ModelInfo[]) => void;
  /** Закрыть меню перед показом модалки; на странице каталога закрывать нечего. */
  onBeforeDialog?: () => void;
};

/**
 * Скачивание, удаление и переключение моделей — одним набором на всё
 * приложение.
 *
 * Выпадающий список в настройках и страница каталога делают с моделями одно
 * и то же, но выглядят по-разному. Разъехавшиеся копии этой логики означали
 * бы, что модель, скачанная из одного места, ведёт себя не так, как та же
 * модель, скачанная из другого, — поэтому вся она живёт здесь, а разметка
 * остаётся за вызывающим.
 */
export function useModelActions({ models, value, language, onConfigChanged, onModelsChanged, onBeforeDialog }: Params) {
  // Списки, а не по одному идентификатору: загрузок может идти несколько, и
  // завершение первой снимало отметку «занято» со всех остальных — карточка
  // второй снова предлагала «Скачать» посреди её собственной загрузки.
  const [deleting, setDeleting] = useState<string[]>([]);
  const [downloading, setDownloading] = useState<string[]>([]);
  const [status, setStatus] = useState<ModelOperationStatus | null>(null);
  const [pendingDownload, setPendingDownload] = useState<ModelInfo | null>(null);
  const [pendingDelete, setPendingDelete] = useState<ModelInfo | null>(null);
  const [pendingSelect, setPendingSelect] = useState<ModelInfo | null>(null);
  const toastDismissTimer = useRef<number | null>(null);
  const toastRemoveTimer = useRef<number | null>(null);
  // Модели, отмену которых уже попросили. Байты, отправленные до того, как
  // скачиватель заметил флаг, ещё идут — и без этой отметки они возвращали
  // бы тосту «Скачиваю…» и кнопку отмены поверх уже нажатой.
  const cancelRequested = useRef<Set<string>>(new Set());
  // Тот же список, что и в состоянии, но доступный синхронно: состояние
  // обновляется к следующему кадру, а два быстрых клика по одной кнопке
  // случаются в одном.
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

  // Esc закрывает подтверждение. `useOutsideClose` тут не помощник: к моменту
  // показа модалки меню уже закрыто, и его слушатель снят.
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

  // Язык у одноязычной модели прижимается к её языку тем же патчем, которым
  // сохраняется сама модель: отдельная запись оставила бы окно, в котором
  // конфиг ссылается на невозможную пару.
  function persistWithLanguageRule(model: ModelInfo) {
    const next = fallbackLanguage(model, language);
    return (patch: Partial<ConfigResult>) => onConfigChanged(
      next ? { ...patch, language: next } : patch,
    );
  }

  /**
   * Клик по модели.
   *
   * Спрашиваем до переключения, а не после: смена модели выгружает из
   * памяти прежнюю и занимает памятью новую — на слабой машине это секунды
   * ожидания, и случайно ткнув мимо, пользователь получал бы их без всякой
   * причины.
   */
  function requestSelect(model: ModelInfo) {
    // Идёт своя загрузка — клик по карточке молчит: предлагать скачать то,
    // что уже качается, значит открыть окно, кнопка в котором ничего не
    // сделает.
    if (inFlight.current.has(model.id)) return;
    // Клик по уже выбранной модели ничего не меняет — и спрашивать не о чем.
    if (model.id === value) return;
    onBeforeDialog?.();
    // Не скачанную модель движок активировать не может, и раньше клик по ней
    // заканчивался красной строкой «model tiny not downloaded». Но выбор
    // модели — это намерение ею пользоваться, а не ошибка: спрашиваем,
    // качать ли, вместо того чтобы отчитываться о невозможном.
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
   * Отмена скачивания.
   *
   * Тост не закрываем: команда возвращается не мгновенно, и пропавший тост
   * выглядел бы как «отменилось», когда байты ещё идут. Итог покажет сам
   * `startDownload`, дождавшись ответа.
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
    // Вторая загрузка той же модели писала бы тот же `*.part`; бэкенд её
    // тоже отклонит, но объяснять пользователю ошибку, которую мы сами и
    // допустили, — плохой способ её не допускать.
    if (inFlight.current.has(model.id)) return;
    inFlight.current.add(model.id);
    cancelRequested.current.delete(model.id);
    setDownloading((current) => (current.includes(model.id) ? current : [...current, model.id]));
    showStatus({ kind: "loading", text: t("Скачиваю {p0}", { p0: model.label }), detail: model.size, progress: null, cancelModel: model.id });
    try {
      // `null` — отменено: пользователь получил ровно то, что просил, и
      // это не ошибка.
      const outcome = await tauriInvoke<unknown>("download_model", { model: model.id });
      if (outcome == null) {
        showStatus({ kind: "info", text: t("Загрузка отменена"), detail: model.label }, 5000);
        return;
      }
      const next = await invoke<ModelInfo[]>("list_models");
      onModelsChanged(next);
      // Скачали — значит, хотели пользоваться: включаем сразу.
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
    /** Занята ли карточка этой модели своей собственной операцией. */
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
 * Тост прогресса и оба подтверждения. Рендерится через портал, поэтому
 * вызывающему безразлично, где в разметке он это поставит.
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
          {/* Пока идёт скачивание, крестик отменяет само скачивание, а не
              прячет тост. Раньше рядом стояли оба: кнопка «Отменить» и
              крестик со значением «пусть качается, но с глаз долой». Второе
              никто не искал, а два похожих действия в одном углу стоили
              дороже, чем спасали. Подпись у крестика меняется вместе с
              действием, чтобы отмену не нажимали вслепую. */}
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
          {/* Крестик здесь лишний: «Отмена» рядом делает то же самое, а
              две кнопки закрытия в окне из одного вопроса — это выбор без
              разницы. */}
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
              {/* Своё и скачанное удаляются одинаково, а последствия разные:
                  каталожную модель приложение вернёт само, чужой файл —
                  никогда. Разговор об этом идёт здесь, до удаления. */}
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
