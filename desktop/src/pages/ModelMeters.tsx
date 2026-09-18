import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useAnchoredMenu } from "../components/anchoredMenu";
import { useOutsideClose } from "../components/CustomSelect";
import type { ModelAssessment } from "../bridge/modelAssessments";
import { resetModelAssessment } from "../bridge/modelAssessments";
import { t } from "../i18n";
import { assessmentText, meterPercent } from "./modelAssessment";

export function ModelMeters({ id, value, onRefresh }: { id: string; value?: ModelAssessment; onRefresh: () => void }) {
  const [active, setActive] = useState<string | null>(null);
  const open = active !== null;
  const panelId = useId();
  const [resetting, setResetting] = useState(false);
  const [error, setError] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement | null>(null);
  const { menuRef, style, placed } = useAnchoredMenu(open, anchor, 290, "end");
  useOutsideClose(open, anchor, () => setActive(null), menuRef);
  useEffect(() => {
    if (open && placed) menuRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
  }, [open, active, placed, menuRef]);
  const text = assessmentText(value);
  const meters = [
    { id: "speed", label: t("Скорость"), score: value?.speed.score, text: text.speed, warning: false },
    { id: "memory", label: t("Запас памяти"), score: value?.memory.score, text: text.memory, warning: value?.memory.status === "low" },
  ];
  return <div className="model-meters" ref={anchor} onClick={(event) => event.stopPropagation()} onKeyDown={(event) => {
    if (event.key === "Escape") { setActive(null); trigger.current?.focus(); }
    event.stopPropagation();
  }}>
    {meters.map((meter) => {
      const percent = meterPercent(meter.score);
      return <button key={meter.id} title={open ? undefined : meter.text} type="button" className="model-meter" data-warning={meter.warning || undefined} aria-expanded={active === meter.id} aria-controls={open ? panelId : undefined}
          aria-label={`${meter.label}: ${percent === null ? t("Нет оценки") : t("{p0} из 100", { p0: percent })}. ${meter.text}`}
          onClick={(event) => { trigger.current = event.currentTarget; setActive(active === meter.id ? null : meter.id); setError(false); }}>
          <span>{meter.label}</span>
          <span className="model-meter__track" aria-hidden="true">
            {percent === null ? <span className="model-meter__unknown"/> : <span className="model-meter__fill" style={{ width: `${percent}%` }}/>}</span>
        </button>;
    })}
    {open && createPortal(<div id={panelId} className="custom-select__menu model-meter-detail" ref={menuRef} style={style} role="region" aria-label={t("Оценка модели")}>
      <p>{active === "memory" ? text.memory : text.speed}</p><p>{active === "memory" ? text.speed : text.memory}</p><p className="muted">{text.compute}</p>
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
