import { useEffect, useRef, useState } from "react";
import { invoke } from "../bridge";
import { Card, CardHead } from "../components/Shell";
import { t } from "../i18n";
import { ISSUES_URL, issueUrl } from "./feedback";

export function FeedbackCard() {
  const trigger = useRef<HTMLButtonElement>(null);
  const [show, setShow] = useState(false);
  const [status, setStatus] = useState("");
  async function open(url: string) {
    try {
      await invoke("open_url", { url });
      setStatus("");
      return true;
    } catch { setStatus(t("Не удалось открыть браузер. Попробуйте ещё раз.")); return false; }
  }
  return <Card pad="default" className="feedback-card">
    <CardHead title={t("Обратная связь")}/>
    <p>{t("Сообщайте о проблемах и предлагайте улучшения через GitHub. Нужен аккаунт GitHub.")}</p>
    <div className="flex-row feedback-actions">
      <button className="btn btn--primary" ref={trigger} onClick={() => setShow(true)}>{t("Сообщить о проблеме")}</button>
      <button className="btn btn--ghost" onClick={() => void open(issueUrl("feature"))}>{t("Предложить улучшение")}</button>
      <button className="btn btn--ghost" onClick={() => void open(ISSUES_URL)}>{t("Посмотреть известные проблемы")}</button>
    </div>
    <p role="status">{status}</p>
    {show && <ReportDialog onClose={() => { setShow(false); requestAnimationFrame(() => trigger.current?.focus()); }} onOpen={open}/>}
  </Card>;
}

function ReportDialog({ onClose, onOpen }: { onClose: () => void; onOpen: (url: string) => Promise<boolean> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const summaryRequest = useRef<Promise<string> | null>(null);
  const [summary, setSummary] = useState<string | null>(null);
  const [include, setInclude] = useState(true);
  const [logs, setLogs] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  useEffect(() => {
    let active = true;
    summaryRequest.current ??= invoke<string>("get_public_diagnostics");
    void summaryRequest.current.then(value => { if (active) setSummary(value); })
      .catch(() => { if (active) { setSummary(""); setStatus(t("Не удалось собрать сводку. Можно продолжить без неё.")); } });
    return () => { active = false; };
  }, []);
  async function action(work: () => Promise<void>) {
    setBusy(true); setStatus("");
    try { await work(); } catch { setStatus(t("Не удалось выполнить действие. Попробуйте ещё раз.")); }
    finally { setBusy(false); }
  }
  return <dialog className="modal feedback-dialog" ref={dialog} onCancel={onClose} onKeyDown={event => {
    if (event.key !== "Tab") return;
    const controls = Array.from(dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), [tabindex="0"]') ?? []);
    const current = controls.indexOf(document.activeElement as HTMLElement);
    const next = event.shiftKey ? (current <= 0 ? controls.length - 1 : current - 1) : (current + 1) % controls.length;
    event.preventDefault();
    controls[next]?.focus();
  }} aria-labelledby="feedback-title">
    <div className="modal__head"><h2 id="feedback-title">{t("Сообщить о проблеме")}</h2>
      <button className="btn btn--ghost" onClick={onClose}>{t("Закрыть")}</button></div>
    <div className="modal__body">
      <p>{t("Обращение и вложения будут публичными. GitHub загружает файл сразу после его выбора, до публикации обращения.")}</p>
      <label className="checkbox-row"><input type="checkbox" className="checkbox" checked={include} onChange={event => setInclude(event.target.checked)}/>{t("Добавить техническую информацию")}</label>
      <pre className="feedback-preview">{summary === null ? t("Подготовка…") : summary || t("Техническая информация недоступна")}</pre>
      <button className="btn btn--ghost" disabled={busy || !summary} onClick={() => void action(async () => {
        await navigator.clipboard.writeText(summary ?? ""); setStatus(t("Скопировано"));
      })}>{t("Скопировать сводку")}</button>
      <p>{t("Экспорт содержит время, уровень, модуль событий и известные замеры задержек из последних 256 КБ текущего лога. Тексты сообщений, пути и записи голоса исключены.")}</p>
      <button className="btn btn--ghost" disabled={busy} onClick={() => void action(async () => {
        setLogs(await invoke<string>("get_public_logs"));
      })}>{t("Подготовить очищенные логи")}</button>
      {logs !== null && <>
        <pre className="feedback-preview" tabIndex={0}>{logs}</pre>
        <button className="btn btn--ghost" disabled={busy} onClick={() => void action(async () => {
          const saved = await invoke<boolean>("save_public_logs", { content: logs });
          if (saved) setStatus(t("Файл сохранён. Прикрепите его вручную к обращению на GitHub."));
        })}>{t("Сохранить очищенные логи")}</button>
      </>}
      <p role="status" aria-live="polite">{busy ? t("Подготовка…") : status}</p>
    </div>
    <div className="modal__foot"><button className="btn btn--primary" disabled={busy} onClick={() => void action(async () => {
      const opened = await onOpen(issueUrl("bug", include ? summary ?? "" : ""));
      if (!opened) setStatus(t("Не удалось открыть браузер. Попробуйте ещё раз."));
    })}>{t("Продолжить на GitHub")}</button></div>
  </dialog>;
}
