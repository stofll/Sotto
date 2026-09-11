import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useAnchoredMenu } from "../components/anchoredMenu";
import { useOutsideClose } from "../components/CustomSelect";
import type { ModelAssessment } from "../bridge/modelAssessments";
import { resetModelAssessment } from "../bridge/modelAssessments";
import { t } from "../i18n";
import { assessmentText, meterPercent } from "./modelAssessment";

export function ModelMeters({ id, value, onRefresh }: { id: string; value?: ModelAssessment; onRefresh: () => void }) {
  const [open, setOpen] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [error, setError] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement | null>(null);
  const { menuRef, style, placed } = useAnchoredMenu(open, anchor, 290, "end");
  useOutsideClose(open, anchor, () => setOpen(false), menuRef);
  useEffect(() => {
    if (open && placed) menuRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
  }, [open, placed, menuRef]);
  const text = assessmentText(value);
  const meters = [
    { label: t("Скорость"), score: value?.speed.score, text: text.speed, warning: false },
    { label: t("Запас памяти"), score: value?.memory.score, text: text.memory, warning: value?.memory.status === "low" },
  ];
  return <div className="model-meters" ref={anchor} onClick={(event) => event.stopPropagation()} onKeyDown={(event) => {
    if (event.key === "Escape") { setOpen(false); trigger.current?.focus(); }
    event.stopPropagation();
  }}>
    {meters.map((meter) => {
      const percent = meterPercent(meter.score);
      return <button key={meter.label} title={open ? undefined : meter.text} type="button" className="model-meter" data-warning={meter.warning || undefined} aria-expanded={open}
          aria-label={`${meter.label}: ${percent === null ? t("Нет оценки") : t("{p0} из 100", { p0: percent })}. ${meter.text}`}
          onClick={(event) => { trigger.current = event.currentTarget; setOpen(!open); setError(false); }}>
          <span>{meter.label}</span>
          <span className="model-meter__track" aria-hidden="true">
            {percent === null ? <span className="model-meter__unknown">—</span> : <span className="model-meter__fill" style={{ width: `${percent}%` }}/>}</span>
        </button>;
    })}
    {open && createPortal(<div className="custom-select__menu model-meter-detail" ref={menuRef} style={style} role="region" aria-label={t("Оценка модели")}>
      <p>{text.speed}</p><p>{text.memory}</p><p className="muted">{text.compute}</p>
      {value?.load_failed && <p className="text-warn">{t("Последняя загрузка модели не удалась. Попробуйте снова.")}</p>}
      {error && <p role="alert">{t("Не удалось сбросить оценку. Попробуйте снова.")}</p>}
      <div className="model-meter-detail__actions">
        <button type="button" className="btn btn--sm" onClick={onRefresh}>{t("Обновить")}</button>
        <button type="button" className="btn btn--sm" disabled={resetting} onClick={async () => {
          setResetting(true); setError(false);
          try { await resetModelAssessment(id); onRefresh(); }
          catch { setError(true); }
          finally { setResetting(false); }
        }}>{t("Сбросить замеры")}</button>
      </div>
    </div>, document.body)}
  </div>;
}
