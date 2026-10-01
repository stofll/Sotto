import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ConfigChange, ConfigPatch, ConfigResult, MicrophoneResult, ModelInfo, RuntimeStatusResult } from "../bridge/types";
import { Card, type DownloadProgress, type TabId } from "../components/Shell";
import { Icon } from "../components/Icon";
import { AccessibilityNotice } from "../components/AccessibilityNotice";
import { TelemetryControl } from "../pages/settings/TelemetryControl";
import { hasTelemetryDecision } from "../pages/telemetrySettings";
import { HotkeyDisplay, RecordingModeSegmented } from "../pages/settings/CaptureSection";
import { MicPicker } from "../pages/settings/MicrophoneSection";
import { UiLanguagePicker } from "../pages/settings/LanguageSection";
import { ModelActionOverlays, useModelActions } from "../pages/modelActions";
import { downloadSpaceText } from "../pages/modelAssessment";
import { useModelAssessments } from "../pages/useModelAssessments";
import { DEFAULT_HOTKEY, hotkeyParts } from "../hotkey";
import { invoke } from "../bridge";
import { t, useLocale } from "../i18n";
import { onboardingExitTab, onboardingModels, onboardingStep } from "./modelChoices";
import "./onboarding.css";

export type OnboardingProps = {
  active: boolean;
  toolbarTarget: HTMLDivElement | null;
  config: ConfigResult;
  models: ModelInfo[];
  microphones: MicrophoneResult[];
  runtime: RuntimeStatusResult | null;
  progress: DownloadProgress | null;
  onConfigChanged: (change: ConfigChange, onError?: (message: string) => void) => Promise<ConfigResult | null>;
  onModelsChanged: (models: ModelInfo[]) => void;
  onNavigate: (tab: TabId) => void;
  onToggleTheme: () => void;
  /** Whether the post-introduction card is on screen; it stands in for the
   *  window's missing-model warning. */
  onCardShown?: (shown: boolean) => void;
  /** Nothing is left to show or finish: the window may unmount the introduction. */
  onEnd: () => void;
};

function modelDescription(model: ModelInfo) {
  if (model.id === "gigaam-v3") return t("Для русской речи. Сама ставит знаки, текст приходит в конце фразы.");
  if (model.id === "nemotron-streaming") return t("Потоковая: слова появляются, пока вы говорите. Русский и английский.");
  if (model.id === "turbo") return t("Если языков много. Может распознавать на видеокарте.");
  return t("Модель, которой вы пользуетесь сейчас.");
}

function ModelChoice({ model, selected, disabled, onChoose }: { model: ModelInfo; selected: boolean; disabled: boolean; onChoose: () => void }) {
  const id = useId();
  return <Card pad="rows" className={`onboarding__model${selected ? " onboarding__model--selected" : ""}`}>
    <label>
      <input type="radio" name="onboarding-model" value={model.id} checked={selected} disabled={disabled} aria-labelledby={`${id}-name`} aria-describedby={`${id}-about`} onChange={onChoose}/>
      <h2 id={`${id}-name`}>{model.label}</h2>
      <span className="mono">{model.size}</span>
      <p id={`${id}-about`}>{modelDescription(model)}</p>
      {model.downloaded && <span className="tag">{t("Скачана")}</span>}
    </label>
  </Card>;
}

/** Remains mounted after completion while its banner or a background download
 *  still has something to show, then ends the session through `onEnd`. */
export default function Onboarding(props: OnboardingProps) {
  const { active, toolbarTarget, config, models, microphones, runtime, progress, onConfigChanged, onModelsChanged, onNavigate, onToggleTheme, onCardShown, onEnd } = props;
  const locale = useLocale();
  const step = onboardingStep(config.onboarding_step);
  const cloud = config.ai_processing?.pipeline_mode === "cloud";
  // A fresh installation has no speech language yet; the interface language is
  // the best guess of what the user will dictate.
  const choices = onboardingModels(models, config.language ?? (locale === "en" ? "en" : "ru"), runtime?.os, config.model);
  // The choice shows and counts at once; its write may still be queued when
  // the user presses the download button.
  const [picked, setPicked] = useState<string | null>(null);
  const chosen = choices.find((model) => model.id === (picked ?? config.onboarding_model ?? config.model)) ?? choices[0];
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  const [catalogFailed, setCatalogFailed] = useState(false);
  const [banner, setBanner] = useState(false);
  const [moving, setMoving] = useState(() => document.hasFocus() && document.visibilityState !== "hidden");
  const saving = useRef(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const bannerCopy = useRef<HTMLDivElement>(null);
  const assessments = useModelAssessments(models.map((model) => `${model.id}:${model.downloaded}`).join(","), active && step === 1);
  const actions = useModelActions({
    models, value: config.model, language: config.language, onModelsChanged, trackOwnDownloadsOnly: true,
    onConfigChanged: (patch) => onConfigChanged({
      ...patch,
      ...(cloud ? { ai_processing: { pipeline_mode: "local" } } : {}),
    }),
  });
  // A download started on the Models page counts too; that page activates it.
  const inFlight = actions.activeDownloads;
  const downloading = inFlight.length > 0;
  const idle = !active && !banner && !downloading && !actions.status;

  useEffect(() => { if (active) heading.current?.focus(); }, [active, step]);
  useEffect(() => { if (!active && banner) bannerCopy.current?.focus(); }, [active, banner]);
  useEffect(() => { if (idle) onEnd(); }, [idle, onEnd]);
  useEffect(() => {
    onCardShown?.(!active && banner);
    return () => onCardShown?.(false);
  }, [active, banner, onCardShown]);
  useEffect(() => {
    if (!active) return;
    const pause = () => setMoving(false);
    const resume = () => setMoving(document.hasFocus() && document.visibilityState !== "hidden");
    document.addEventListener("visibilitychange", resume);
    window.addEventListener("blur", pause);
    window.addEventListener("focus", resume);
    return () => {
      document.removeEventListener("visibilitychange", resume);
      window.removeEventListener("blur", pause);
      window.removeEventListener("focus", resume);
    };
  }, [active]);

  async function save(patch: ConfigPatch) {
    if (saving.current) return null;
    saving.current = true;
    setBusy(true);
    setFailed(null);
    let reason = "";
    const result = await onConfigChanged(patch, (message) => { reason = message; });
    if (!result) setFailed(reason);
    saving.current = false;
    setBusy(false);
    return result;
  }

  async function finish() {
    // Shown before the write lands, so the finished flow never renders a frame
    // with neither the introduction nor its banner.
    setBanner(true);
    if (!await save({ onboarding_completed: true })) { setBanner(false); return; }
    onNavigate(onboardingExitTab(config, models, downloading));
  }

  async function next() {
    if (step !== 3) await save({ onboarding_step: step + 1 });
    // Leaving the last step answers its telemetry question, even unticked, so
    // the main window does not ask it again.
    else if (hasTelemetryDecision(config.telemetry_enabled) || await save({ telemetry_enabled: false })) await finish();
  }

  async function choose(model: ModelInfo) {
    setFailed(null);
    setPicked(model.id);
    // Not `save`: disabling the radios mid-write would drop keyboard focus.
    let reason = "";
    if (!await onConfigChanged({ onboarding_model: model.id }, (message) => { reason = message; })) {
      setPicked(null);
      setFailed(reason);
    }
  }

  async function begin() {
    if (!chosen || saving.current) return;
    if (chosen.downloaded) {
      // The current local model already works; reloading it only costs time.
      if (chosen.id !== config.model || cloud) {
        saving.current = true;
        setBusy(true);
        const ok = await actions.selectModel(chosen, cloud);
        saving.current = false;
        setBusy(false);
        if (!ok) return;
      }
    } else if (!inFlight.includes(chosen.id)) {
      // Persist the choice before launching. The download itself does not hold
      // navigation and continues if the window is hidden or the flow is skipped.
      if (!await save({ onboarding_model: chosen.id })) return;
      void actions.startDownload(chosen);
    }
    await save({ onboarding_step: 2 });
  }

  if (!active) {
    if (!banner) return <ModelActionOverlays actions={actions}/>;
    const model = models.find((item) => item.id === (inFlight[0] ?? config.model));
    const ready = runtime?.active_engine === "cloud-stt"
      || (model?.downloaded && (runtime?.loaded_model === model.id || runtime?.model_loads_on_demand));
    const downloadFailed = actions.status?.kind === "error";
    return <Card pad="rows" className="window-banner">
      <div className="window-banner__copy" role={downloadFailed ? "alert" : "status"} ref={bannerCopy} tabIndex={-1}>
        <strong>{downloading ? t("Скачивается {p0}", { p0: model?.label ?? "" }) : ready ? t("Можно диктовать") : t("Для диктовки нужна модель")}</strong>
        <p>{downloadFailed ? actions.status?.text : downloading
          ? t("Пока модель скачивается, поставьте курсор в любое текстовое поле. После загрузки нажмите {p0}.", { p0: config.hotkey || DEFAULT_HOTKEY })
          : ready ? t("Поставьте курсор в любое текстовое поле и нажмите {p0}.", { p0: config.hotkey || DEFAULT_HOTKEY })
          : t("Скачайте модель в разделе «Модели», затем нажмите горячую клавишу.")}</p>
        {downloading && <progress aria-label={t("Скачивание модели")} max={progress?.total ?? undefined} value={progress?.total && progress.model === model?.id ? progress.downloaded : undefined}/>}
      </div>
      {downloading ? <button className="btn btn--ghost" type="button" onClick={() => void actions.cancelDownload(inFlight[0])}>{t("Отменить скачивание")}</button>
        : (!ready || downloadFailed) && <button className="btn btn--primary" type="button" onClick={() => onNavigate("models")}>{t("Открыть модели")}</button>}
      <button className="btn btn--ghost btn--icon" type="button" aria-label={t("Закрыть подсказку")} onClick={() => setBanner(false)}><Icon name="x" size={14}/></button>
    </Card>;
  }

  const titles = [t("Нажали. Сказали. Текст уже там."), t("Одна модель, чтобы начать"), t("Ваше сочетание"), t("Можно не трогать")];
  const cloudNote = t("У вас настроено облачное распознавание: запись отправляется выбранному сервису. Выбор локальной модели вернёт распознавание на этот компьютер.");
  const leads = [
    cloud ? cloudNote : null,
    t("Выберите модель для своего языка. Модель не того языка может распознать бессмыслицу. Остальные модели останутся в каталоге."),
    t("Сочетание уже работает. Можно оставить его или выбрать своё."),
    t("Всё это можно поменять позже в настройках."),
  ];
  const disk = chosen ? assessments.values[chosen.id] : undefined;
  const insufficient = !chosen?.downloaded && disk?.download?.insufficient;
  const chosenDownloading = !!chosen && inFlight.includes(chosen.id);
  const primary = busy ? t("Сохранение…")
    : step === 3 ? t("Начать диктовать")
    : step !== 1 || chosenDownloading ? t("Дальше")
    : !chosen?.downloaded ? (cloud ? t("Скачать и перейти на локальную") : t("Скачать и продолжить"))
    : cloud ? t("Перейти на локальную модель") : t("Дальше");
  return <div className="onboarding" data-moving={moving} data-testid="onboarding">
    {toolbarTarget && createPortal(<div className="onboarding__toolbar">
      <UiLanguagePicker value={config.ui_language} onConfigChanged={onConfigChanged}/>
      <button className="btn btn--ghost btn--icon" type="button" aria-label={t("Переключить тему")} onClick={onToggleTheme}><Icon name={config.theme === "light" ? "sun" : "moon"} size={15}/></button>
      <button className="btn btn--ghost" type="button" disabled={busy} onClick={() => void finish()}>{t("Пропустить введение")}</button>
    </div>, toolbarTarget)}
    <div className="onboarding__stage" key={step}>
      <header className="onboarding__heading">
        <span className="onboarding__counter">{t("Шаг {p0} из {p1}", { p0: step + 1, p1: 4 })}</span>
        <h1 ref={heading} tabIndex={-1}>{titles[step]}</h1>
        {leads[step] && <p>{leads[step]}</p>}
      </header>
      {step === 0 && <div className="onboarding__grid">
        <Card pad="rows"><Icon name="play" size={28}/><h2>{t("Сочетание")}</h2><div className="onboarding__keys">{hotkeyParts(config.hotkey).map((key, index) => <kbd className="kbd" key={index}>{key}</kbd>)}</div><p>{config.recording_mode === "push_to_talk" ? t("Говорите, удерживая сочетание. Отпустите, чтобы закончить.") : t("Одно нажатие начинает запись, второе заканчивает.")}</p></Card>
        <Card pad="rows"><div className="onboarding__speech" aria-hidden="true">{[2, 4, 6, 8, 6, 4, 2].map((height, index) => <i key={index} style={{ height: height * 4 }}/>)}</div><h2>{t("Речь")}</h2><p>{cloud ? t("Сейчас запись отправляется выбранному облачному сервису.") : t("Говорите как обычно. При локальном распознавании голос никуда не отправляется.")}</p></Card>
        <Card pad="rows"><Icon name="text" size={28}/><h2>{t("Текст в поле")}</h2><p>{t("Готовый текст появляется там, где курсор.")}</p></Card>
      </div>}
      {step === 1 && <>
        <div className="onboarding__grid" role="radiogroup" aria-label={t("Модель распознавания")}>
          {choices.map((model) => <ModelChoice key={model.id} model={model} selected={chosen?.id === model.id} disabled={downloading} onChoose={() => void choose(model)}/>)}
        </div>
        {!choices.length && <div>
          <p className="inline-error" role="alert">{t("Не удалось получить список моделей. Откройте каталог или попробуйте ещё раз.")}</p>
          <button className="btn btn--ghost" type="button" onClick={() => { setCatalogFailed(false); void invoke<ModelInfo[]>("list_models").then(onModelsChanged).catch(() => setCatalogFailed(true)); }}>{t("Повторить")}</button>
          {catalogFailed && <p className="inline-error" role="alert">{t("Не удалось загрузить модели.")}</p>}
        </div>}
        {cloud && <p className="onboarding__note">{cloudNote}</p>}
        <p className="onboarding__note">{t("Скачивание пойдёт в фоне. «Пропустить шаг» не начинает загрузку.")}</p>
        {insufficient && <p className="inline-error" role="alert">{downloadSpaceText(disk)}</p>}
      </>}
      {step === 2 && <div className="card-stack">
        <Card pad="rows"><div className="capture-row"><div className="set-cell"><span className="set-label">{t("Горячая клавиша")}</span><HotkeyDisplay hotkey={config.hotkey} onConfigChanged={onConfigChanged}/></div><div className="vrule"/><div className="set-cell"><span className="set-label">{t("Режим записи")}</span><RecordingModeSegmented value={config.recording_mode ?? "toggle"} onConfigChanged={onConfigChanged}/></div></div></Card>
        <Card pad="rows"><span className="set-label">{t("Микрофон")}</span><MicPicker microphone={config.microphone} microphones={microphones} onConfigChanged={onConfigChanged}/><p>{t("Проверьте микрофон сейчас, чтобы получить системное разрешение до первой диктовки.")}</p></Card>
        {runtime?.os === "macos" && <AccessibilityNotice/>}
      </div>}
      {step === 3 && <Card pad="rows" className="onboarding__options">
        {!runtime?.portable && <label className="checkbox-row"><input type="checkbox" className="checkbox" checked={config.auto_start ?? false} onChange={(event) => void onConfigChanged({ auto_start: event.target.checked })}/>{t("Запускать вместе с системой")}</label>}
        <label className="checkbox-row"><input type="checkbox" className="checkbox" checked={config.sound_feedback ?? true} onChange={(event) => void onConfigChanged({ sound_feedback: event.target.checked })}/>{t("Звук начала и конца записи")}</label>
        <div><strong>{t("LLM-обработка")}</strong><p>{t("Необязательная обработка текста требует своего ключа. Настроить её можно позже в разделе «Провайдеры и ключи».")}</p></div>
        <div><TelemetryControl value={config.telemetry_enabled} onConfigChanged={onConfigChanged}/><p>{t("Перед включением покажем состав событий. Записи и текст не отправляются.")}</p></div>
      </Card>}
      {failed !== null && <p className="inline-error" role="alert">{t("Не удалось сохранить настройку: {p0}", { p0: failed })}</p>}
      <footer className="onboarding__actions">
        {step > 0 && <button className="btn btn--ghost" type="button" disabled={busy} onClick={() => void save({ onboarding_step: step - 1 })}>{t("Назад")}</button>}
        {step > 0 && <button className="btn btn--ghost" type="button" disabled={busy} onClick={() => void next()}>{t("Пропустить шаг")}</button>}
        <button className="btn btn--primary" type="button" disabled={busy || (step === 1 && (!chosen || !!insufficient))} onClick={() => void (step === 1 ? begin() : next())}>{primary}</button>
      </footer>
    </div>
    <svg className="onboarding__waves" viewBox="0 0 1440 600" preserveAspectRatio="none" aria-hidden="true">
      <path d="M-720 300 C-360 100 0 500 360 300 S1080 100 1440 300 S2160 500 2520 300 L2520 650 H-720Z"/>
      <path d="M-720 360 C-360 180 0 540 360 360 S1080 180 1440 360 S2160 540 2520 360 L2520 650 H-720Z"/>
      <path d="M-720 430 C-360 270 0 590 360 430 S1080 270 1440 430 S2160 590 2520 430 L2520 650 H-720Z"/>
    </svg>
    <ModelActionOverlays actions={actions}/>
  </div>;
}
