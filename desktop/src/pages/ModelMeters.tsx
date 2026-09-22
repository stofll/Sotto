import { Hint } from "../components/Hint";
import type { ModelAssessment } from "../bridge/modelAssessments";
import { t } from "../i18n";
import { speedPresentation } from "./modelAssessment";

export function ModelMeters({ value }: { value?: ModelAssessment }) {
  const speed = speedPresentation(value?.speed.score);
  return <div className="model-meters" onClick={(event) => event.stopPropagation()} onKeyDown={(event) => event.stopPropagation()}>
    <Hint asChild text={speed.hint}><span className="model-meter model-meter--speed" tabIndex={0} role="img" aria-label={`${t("Скорость")}: ${speed.label}`}>
      <span aria-hidden="true">{t("Скорость")}</span>
      <span className="model-meter__track" aria-hidden="true">
        {speed.fill === null ? <span className="model-meter__unknown"/> : <span className="model-meter__fill" style={{ width: `${speed.fill}%` }}/>}</span>
    </span></Hint>
  </div>;
}
