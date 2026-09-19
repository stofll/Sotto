import { Card, PageHeader } from "../components/Shell";
import type { ConfigResult, MicrophoneResult, ModelInfo } from "../bridge/types";
import { t } from "../i18n";
import { DEFAULT_HOTKEY } from "../hotkey";
import { fallbackModels } from "./modelCatalog";
import { HotkeyDisplay, RecordingModeSegmented } from "./settings/CaptureSection";
import { LanguagePicker, UiLanguagePicker } from "./settings/LanguageSection";
import { MicPicker } from "./settings/MicrophoneSection";
import { BehaviorSection } from "./settings/BehaviorSection";
import { Icon } from "../components/Icon";
import { OverlaySettings } from "./OverlaySettings";
import { AdvancedSection } from "./settings/AdvancedSection";
import { SetLabel } from "./settings/controls";

type Props = {
  config: ConfigResult | null;
  microphones: MicrophoneResult[];
  models: ModelInfo[];
  /** A portable copy does not manage autostart — see the row itself. */
  portable?: boolean;
  onConfigChanged: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null>;
};

export function SettingsPage({ config, microphones, models, portable, onConfigChanged }: Props) {
  const model = config?.model || "large-v3";
  // Settings need only the selected model — the speech language and the device
  // choice follow from its properties (language, CPU-only).
  const selectedModelInfo = (models.length ? models : fallbackModels()).find((item) => item.id === model);
  const recordingMode = config?.recording_mode ?? "toggle";

  return (
    <div className="page">
      <PageHeader title={t("Настройки")}/>

      <div className="card-stack card-stack--rows">
        {/* 1. Capture row: hotkey · recording mode — one row split by a vrule.
            The model picker lives on its own page: here it duplicated the catalog,
            and with it downloading and deleting. */}
        <Card pad="rows">
          <div className="capture-row">
            <div className="set-cell">
              <SetLabel title={t("Горячая клавиша")} hint={t("Диктовка. Пойдёт ли текст в LLM, решает режим обработки на вкладке «ИИ».")}/>
              <HotkeyDisplay hotkey={config?.hotkey} fallback={DEFAULT_HOTKEY} onConfigChanged={onConfigChanged}/>
            </div>
            <div className="vrule"/>
            <div className="set-cell">
              <SetLabel title={t("Режим записи")} hint={t("Переключение\nНажмите горячую клавишу, чтобы начать запись, и нажмите снова, чтобы закончить. Удобно для длинной диктовки.\n\nУдержание\nГоворите, удерживая горячую клавишу, и отпустите её, чтобы закончить. Удобно для коротких фраз.")}/>
              <RecordingModeSegmented value={recordingMode} onConfigChanged={onConfigChanged}/>
            </div>
          </div>
        </Card>

        {/* 2. Languages — 2 cols. The names separate the two settings from each
            other, while the hint on the speech language answers the next
            question: what happens if you dictate in another one. */}
        <Card pad="rows">
          <div className="lang-row">
            <div className="set-cell">
              <SetLabel title={t("Язык речи")} hint={t("Язык, на котором вы диктуете: модель распознаёт речь именно как его. «Авто» определяет язык по самой записи — это чуть медленнее и иногда ошибается на коротких фразах. На язык интерфейса не влияет.")}/>
              <LanguagePicker language={config?.language} model={selectedModelInfo} models={models} onConfigChanged={onConfigChanged}/>
            </div>
            <div className="set-cell">
              <SetLabel title={t("Язык интерфейса")}/>
              <UiLanguagePicker value={config?.ui_language} onConfigChanged={onConfigChanged}/>
            </div>
          </div>
        </Card>

        {/* 3. Microphone — own row (long device names) */}
        <Card pad="rows">
          <div className="set-cell">
            <SetLabel title={t("Микрофон")}/>
            <MicPicker microphone={config?.microphone} microphones={microphones} onConfigChanged={onConfigChanged}/>
          </div>
        </Card>

        {/* 4. Behaviour row: the paste settings as one sequence of checkboxes; a
            separate switch created a false visual hierarchy. */}
        <Card pad="rows">
          <BehaviorSection config={config} onConfigChanged={onConfigChanged}/>
        </Card>

        <details className="card card--rows advanced" data-testid="overlay-disclosure">
          <summary><Icon name="chev-down" size={13}/>{t("Оверлей")}</summary>
          <OverlaySettings config={config} onConfigChanged={onConfigChanged}/>
        </details>

        {/* 5. Everything that is configured once or never. Collapsed on purpose:
            at the top level these controls cost a new user more than they saved an
            experienced one. */}
        <AdvancedSection
          config={config}
          portable={portable}
          cpuOnly={selectedModelInfo?.cpu_only}
          onConfigChanged={onConfigChanged}
        />
      </div>
    </div>
  );
}
