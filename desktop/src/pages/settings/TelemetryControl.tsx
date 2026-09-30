import { useRef, useState } from "react";
import type { ConfigChanged } from "./controls";
import { isTelemetryEnabled } from "../telemetrySettings";
import { t } from "../../i18n";
import { Modal } from "../../components/Modal";

/** Dismissing the explanation accepts the user's original choice to opt out. */
export function TelemetryControl({ value, onConfigChanged }: { value?: boolean; onConfigChanged: ConfigChanged }) {
  const [pending, setPending] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const saving = useRef(false);

  async function save(enabled: boolean) {
    if (saving.current) return;
    saving.current = true;
    setBusy(true);
    setError(null);
    let reason = "";
    const saved = await onConfigChanged({ telemetry_enabled: enabled }, (message) => { reason = message; });
    if (saved) setPending(false);
    else setError(reason);
    saving.current = false;
    setBusy(false);
  }

  const failure = error !== null && <p className="inline-error" role="alert">{t("Не удалось сохранить настройку: {p0}", { p0: error })}</p>;
  return <>
    <label className="checkbox-row">
      <input className="checkbox" type="checkbox" checked={!pending && isTelemetryEnabled(value)} disabled={busy}
        onChange={(event) => { setError(null); if (event.target.checked) void save(true); else setPending(true); }}/>
      {t("Разрешить обезличенную телеметрию")}
    </label>
    {!pending && failure}
    {pending && <Modal title={t("Что отправляет телеметрия")} busy={busy} onClose={() => void save(false)}>
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
        <p>{t("Полный перечень полей описан в документации Sotto о телеметрии.")}</p>
        <p>{t("Отключение действует сразу. Уже отправленные события не удаляются; локальная очередь сохраняется и отправится при повторном включении.")}</p>
        {failure}
      </div>
      <div className="modal__foot">
        <button type="button" className="btn btn--ghost" disabled={busy} onClick={() => { setPending(false); setError(null); }}>{t("Оставить включённой")}</button>
        <button type="button" className="btn btn--primary" disabled={busy} onClick={() => void save(false)}>{t("Выключить")}</button>
      </div>
    </Modal>}
  </>;
}
