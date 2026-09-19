import { Hint } from "../components/Hint";
import { useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useAnchoredMenu } from "../components/anchoredMenu";
import { useOutsideClose } from "../components/CustomSelect";
import type { ModelAssessment } from "../bridge/modelAssessments";
import { t } from "../i18n";
import { assessmentText, meterPercent } from "./modelAssessment";

export function ModelMeters({ value }: { value?: ModelAssessment }) {
  const [active, setActive] = useState<string | null>(null);
  const open = active !== null;
  const panelId = useId();
  const anchor = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement | null>(null);
  const { menuRef, style } = useAnchoredMenu(open, anchor, 290, "end");
  useOutsideClose(open, anchor, () => setActive(null), menuRef);
  const text = assessmentText(value);
  const meters = [
    { id: "speed", label: t("Скорость"), score: value?.speed.score, text: text.speed, warning: false, approximate: value?.speed.approximate === true },
    { id: "memory", label: t("Запас памяти"), score: value?.memory.score, text: text.memory, warning: value?.memory.status === "low", approximate: false },
  ];
  return <div className="model-meters" ref={anchor} onClick={(event) => event.stopPropagation()} onKeyDown={(event) => {
    if (event.key === "Escape") { setActive(null); trigger.current?.focus(); }
    event.stopPropagation();
  }}>
    {meters.map((meter) => {
      const percent = meterPercent(meter.score);
      return <Hint key={meter.id} asChild text={open ? undefined : meter.text}><button type="button" className="model-meter" data-warning={meter.warning || undefined} aria-expanded={active === meter.id} aria-controls={open ? panelId : undefined}
          aria-label={`${meter.label}: ${percent === null ? t("Нет оценки") : t("{p0} из 100", { p0: percent })}. ${meter.text}`}
          onClick={(event) => { trigger.current = event.currentTarget; event.currentTarget.focus(); setActive(active === meter.id ? null : meter.id); }}>
          <span>{meter.label}</span>
          <span className="model-meter__track" aria-hidden="true">
            {percent === null ? <span className="model-meter__unknown"/> : <span className="model-meter__fill" data-approximate={meter.approximate || undefined} style={{ width: `${percent}%` }}/>}</span>
        </button></Hint>;
    })}
    {open && createPortal(<div id={panelId} className="custom-select__menu model-meter-detail" ref={menuRef} style={style} role="region" aria-label={active === "memory" ? t("Запас памяти") : t("Скорость")}>
      <p>{active === "memory" ? text.memory : text.speed}</p>
    </div>, document.body)}
  </div>;
}
