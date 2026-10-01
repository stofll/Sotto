import { useRef, useState, type ReactNode } from "react";
import type { ConfigChanged } from "./controls";
import { isTelemetryEnabled } from "../telemetrySettings";
import { t } from "../../i18n";
import { Card } from "../../components/Shell";
import { Icon } from "../../components/Icon";
import { Modal } from "../../components/Modal";

/** One write of the telemetry answer at a time, with its failure kept for the
 *  control that asked. */
function useTelemetryAnswer(onConfigChanged: ConfigChanged) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const saving = useRef(false);
  async function save(enabled: boolean) {
    if (saving.current) return false;
    saving.current = true;
    setBusy(true);
    setError(null);
    let reason = "";
    const saved = await onConfigChanged({ telemetry_enabled: enabled }, (message) => { reason = message; });
    if (!saved) setError(reason);
    saving.current = false;
    setBusy(false);
    return !!saved;
  }
  const failure = error !== null && <p className="inline-error" role="alert">{t("Не удалось сохранить настройку: {p0}", { p0: error })}</p>;
  return { busy, failure, save, clearError: () => setError(null) };
}

/** Shown before sending starts, so consent covers what the user has read.
 *  Every way out except «Включить» leaves telemetry as it was. */
function TelemetryExplanation({ busy, failure, onEnable, onClose }: { busy: boolean; failure: ReactNode; onEnable: () => void; onClose: () => void }) {
  return <Modal title={t("Что отправляет телеметрия")} busy={busy} onClose={onClose}>
    <div className="modal__body telemetry-explanation">
      <p><strong>{t("Записи, текст, пути, ключи, имя компьютера и микрофона, заголовки окон не отправляются.")}</strong></p>
      <p>{t("По этим событиям видно, какие сборки не вставляют текст и какие модели распознают речь слишком долго.")}</p>
      <p>{t("События включают ОС, архитектуру, канал выпуска и случайные идентификаторы установки и сессии. Они не связаны с аккаунтом или именем компьютера.")}</p>
      <dl>
        <dt>{t("Запуск")}</dt><dd>{t("Версия, язык интерфейса и способ запуска — чтобы видеть, какие сборки используются.")}</dd>
        <dt>{t("Диктовка")}</dt><dd>{t("Микрофон или файл, режим записи и распознавания, модель, процессор или видеокарта, длительность аудио с шагом 10 секунд, время распознавания, примерная длина текста, сэкономленное время, число замен и результат вставки. При LLM-обработке — известный сервис, без адреса и ключа.")}</dd>
        <dt>{t("Сбой и отмена")}</dt><dd>{t("Короткая причина из готового списка, без текста ошибки. Отмена учитывается отдельно от сбоя.")}</dd>
        <dt>{t("Сессия")}</dt><dd>{t("Длительность, число успешных, неудачных и отменённых диктовок, общее время аудио и сэкономленное время — чтобы отличить пробный запуск от регулярного использования.")}</dd>
      </dl>
      <p>{t("Полный перечень полей приведён в документации Sotto о приватности.")}</p>
      <p>{t("Выключить можно в любой момент в «Настройки → Дополнительно». Уже отправленные события при этом не удаляются.")}</p>
      {failure}
    </div>
    <div className="modal__foot">
      <button type="button" className="btn btn--ghost" disabled={busy} onClick={onClose}>{t("Отмена")}</button>
      <button type="button" className="btn btn--primary" disabled={busy} onClick={onEnable}>{t("Включить")}</button>
    </div>
  </Modal>;
}

/** Turning telemetry on goes through the explanation; turning it off is immediate. */
export function TelemetryControl({ value, onConfigChanged }: { value?: boolean; onConfigChanged: ConfigChanged }) {
  const [explaining, setExplaining] = useState(false);
  const { busy, failure, save, clearError } = useTelemetryAnswer(onConfigChanged);
  return <>
    <label className="checkbox-row">
      <input className="checkbox" type="checkbox" checked={isTelemetryEnabled(value)} disabled={busy}
        onChange={(event) => { clearError(); if (event.target.checked) setExplaining(true); else void save(false); }}/>
      {t("Разрешить обезличенную телеметрию")}
    </label>
    {!explaining && failure}
    {explaining && <TelemetryExplanation busy={busy} failure={failure}
      onEnable={() => void save(true).then((saved) => { if (saved) setExplaining(false); })}
      onClose={() => { setExplaining(false); clearError(); }}/>}
  </>;
}

/** The one-time question for an installation that never answered it in the
 *  introduction. The cross only hides it until the next launch; either answer
 *  is stored, and the window stops rendering the card. */
export function TelemetryConsentCard({ onConfigChanged, onDismiss }: { onConfigChanged: ConfigChanged; onDismiss: () => void }) {
  const [explaining, setExplaining] = useState(false);
  const { busy, failure, save, clearError } = useTelemetryAnswer(onConfigChanged);
  return <Card pad="rows" className="window-banner">
    <div className="window-banner__copy" role="status">
      <strong>{t("Помочь улучшить Sotto?")}</strong>
      <p>{t("Телеметрия выключена. Если разрешите, Sotto будет отправлять обезличенные события использования — без записей и текста. Выбор можно поменять в «Настройки → Дополнительно».")}</p>
      {!explaining && failure}
    </div>
    <button className="btn btn--ghost" type="button" disabled={busy} onClick={() => { clearError(); setExplaining(true); }}>{t("Что отправляется")}</button>
    <button className="btn btn--ghost" type="button" disabled={busy} onClick={() => void save(false)}>{t("Не отправлять")}</button>
    <button className="btn btn--primary" type="button" disabled={busy} onClick={() => void save(true)}>{t("Разрешить")}</button>
    <button className="btn btn--ghost btn--icon" type="button" disabled={busy} aria-label={t("Закрыть подсказку")} onClick={onDismiss}><Icon name="x" size={14}/></button>
    {explaining && <TelemetryExplanation busy={busy} failure={failure}
      onEnable={() => void save(true)}
      onClose={() => { setExplaining(false); clearError(); }}/>}
  </Card>;
}
