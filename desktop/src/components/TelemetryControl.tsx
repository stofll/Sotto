import { useRef, useState } from "react";
import type { ConfigChanged } from "../pages/settings/controls";
import { isTelemetryEnabled } from "../pages/telemetrySettings";
import { t } from "../i18n";
import { Modal } from "./Modal";

/** Dismissing the explanation accepts the user's original choice to opt out. */
export function TelemetryControl({ value, onConfigChanged }: { value?: boolean; onConfigChanged: ConfigChanged }) {
  const [pending, setPending] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const saving = useRef(false);

  async function save(enabled: boolean) {
    if (saving.current) return;
    saving.current = true;
    setBusy(true);
    setError(false);
    try {
      const saved = await onConfigChanged({ telemetry_enabled: enabled });
      if (saved) setPending(false);
      else setError(true);
    } catch { setError(true); }
    finally { saving.current = false; setBusy(false); }
  }

  return <>
    <label className="checkbox-row">
      <input className="checkbox" type="checkbox" checked={!pending && isTelemetryEnabled(value)} disabled={busy}
        onChange={(event) => { setError(false); if (event.target.checked) void save(true); else setPending(true); }}/>
      {t("Разрешить обезличенную телеметрию")}
    </label>
    {error && !pending && <p role="alert">{t("Не удалось сохранить настройку. Попробуйте ещё раз.")}</p>}
    {pending && <Modal title={t("Что отправляет телеметрия")} busy={busy} onClose={() => void save(false)}>
      <div className="modal__body telemetry-explanation">
        <p><strong>{t("Записи, текст, пути, ключи, имя компьютера и микрофона, заголовки окон не отправляются.")}</strong></p>
        <p>{t("По этим событиям видно, какие сборки не вставляют текст и какие модели распознают речь слишком долго.")}</p>
        <p>{t("События включают ОС, архитектуру, канал выпуска и случайные идентификаторы установки и сессии. Они не связаны с аккаунтом или именем компьютера.")}</p>
        <dl>
          <dt>{t("Запуск")}</dt><dd>{t("Версия, язык интерфейса и способ запуска — чтобы видеть, какие сборки используются.")}</dd>
          <dt>{t("Диктовка")}</dt><dd>{t("Модель, процессор или видеокарта, длительность с шагом 10 секунд, время распознавания и результат вставки. При LLM-обработке — известный сервис, без адреса и ключа.")}</dd>
          <dt>{t("Сбой и отмена")}</dt><dd>{t("Короткая причина из готового списка, без текста ошибки. Отмена учитывается отдельно от сбоя.")}</dd>
          <dt>{t("Сессия")}</dt><dd>{t("Число диктовок и неудач — чтобы отличить пробный запуск от регулярного использования.")}</dd>
        </dl>
        <p>{t("Отключение действует сразу. Уже отправленные события не удаляются; локальная очередь сохраняется и отправится при повторном включении.")}</p>
        {error && <p role="alert">{t("Не удалось сохранить настройку. Попробуйте ещё раз.")}</p>}
      </div>
      <div className="modal__foot">
        <button type="button" className="btn btn--ghost" disabled={busy} onClick={() => { setPending(false); setError(false); }}>{t("Оставить включённой")}</button>
        <button type="button" className="btn btn--primary" disabled={busy} onClick={() => void save(false)}>{t("Выключить")}</button>
      </div>
    </Modal>}
  </>;
}
