import { useState } from "react";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { CustomSelect } from "../../components/CustomSelect";
import { Icon } from "../../components/Icon";
import { Hint } from "../../components/Hint";
import { HintIcon, type ConfigChanged } from "./controls";
import { t } from "../../i18n";
import type { ConfigResult } from "../../bridge/types";

const SOUND_VOLUME_PRESETS = () => ([
  { label: t("Тихо"), value: 0.15 },
  { label: t("Средне"), value: 0.35 },
  { label: t("Громко"), value: 0.7 },
]);
const DEFAULT_SOUND_VOLUME = 0.35;
// A pseudo-volume for the «Выключено» option. To the user "are the sounds on"
// and "how loud" are one choice, so there is one control: a separate switch cost
// a second click and kept the select disabled while deciding nothing.
const SOUND_OFF = 0;

// Dictation sound cues: recording start, end, text insertion, error. The
// defaults are duplicated in src-tauri/src/sounds.rs.
function SoundFeedbackControl({ enabled, volume, onConfigChanged }: { enabled: boolean; volume: number; onConfigChanged: ConfigChanged }) {
  const presets = SOUND_VOLUME_PRESETS();
  const knownVolume = presets.some((preset) => preset.value === volume) ? volume : DEFAULT_SOUND_VOLUME;
  // We play the chosen value immediately: volume is not picked by ear from the
  // name of a preset.
  function preview(nextVolume: number) {
    void tauriInvoke("preview_sound_cue", { cue: "done", volume: nextVolume }).catch(() => {});
  }

  // «Выключено» does not touch sound_volume: on bringing the cues back the user
  // gets the same volume they chose before.
  function select(next: number) {
    if (next === SOUND_OFF) {
      void onConfigChanged({ sound_feedback: false });
      return;
    }
    void onConfigChanged({ sound_feedback: true, sound_volume: next });
    preview(next);
  }

  return (
    <div className="sound-feedback-control">
      <span className="label-with-hint">
        <span className="sound-feedback-control__label">{t("Звуковые сигналы")}</span>
        <HintIcon text={t("Сигналы отмечают начало записи, завершение, вставку текста и ошибку.")}/>
      </span>
      <CustomSelect
        className="custom-select--sound-volume"
        value={enabled ? knownVolume : SOUND_OFF}
        options={[{ label: t("Выключено"), value: SOUND_OFF }, ...presets]}
        onChange={select}
      />
    </div>
  );
}

export function BehaviorSection({ config, onConfigChanged }: { config: ConfigResult | null; onConfigChanged: ConfigChanged }) {
  const autoPaste = config?.auto_paste ?? true;
  const duckOutput = config?.duck_output_while_recording ?? false;
  const [duckTest, setDuckTest] = useState<"idle" | "running" | "done" | "error">("idle");
  const [duckTestError, setDuckTestError] = useState("");

  async function testOutputDuck() {
    setDuckTest("running");
    setDuckTestError("");
    try {
      await tauriInvoke("preview_output_duck", { level: config?.duck_output_level ?? 0.2 });
      setDuckTest("done");
    } catch (error) {
      setDuckTestError(error instanceof Error ? error.message : String(error));
      setDuckTest("error");
    }
  }
  // One caption for every place the test's state is put into words: the
  // button's hint and the screen-reader line must say the same thing.
  const duckTestLabel = duckTest === "running" ? t("Проверяем приглушение…")
    : duckTest === "done" ? t("Громкость восстановлена")
    : duckTest === "error" ? duckTestError
    : t("Проверить");

  return (
    <div className="behavior-row behavior-row--primary">
      {/* «Пробел в конце» and «Enter после вставки» moved into
          «Дополнительно»: both depend on auto-paste, both are set once for
          a particular scenario — and all three captions together kept the
          row from fitting on a narrow window. */}
      <div className="set-cell behavior-row__paste-options">
        <span className="label-with-hint">
          <label className="checkbox-row">
            <input className="checkbox" type="checkbox" checked={autoPaste} onChange={(e) => void onConfigChanged({ auto_paste: e.target.checked })}/>
            {t("Авто-вставка текста")}
          </label>
          <HintIcon text={t("Сразу вставлять распознанный текст в активное поле. Если выключить, текст останется только в буфере обмена.")}/>
        </span>
      </div>
      <div className="vrule"/>
      <div className="set-cell behavior-row__sound-feedback">
        <SoundFeedbackControl
          enabled={config?.sound_feedback ?? true}
          volume={config?.sound_volume ?? DEFAULT_SOUND_VOLUME}
          onConfigChanged={onConfigChanged}
        />
      </div>
      <div className="vrule"/>
      {/* Ducking is not processing of the recording but what the app does
          to the system while recording; and it is toggled situationally:
          unnecessary with headphones, necessary with speakers. Hence its
          place next to pasting rather than inside «Дополнительно». The
          caption is short: the hint finishes the thought with "while
          recording", and in the row those three words cost exactly the
          space that kept it from fitting. */}
      <div className="set-cell behavior-row__duck">
        <span className="label-with-hint">
          <label className="checkbox-row">
            <input className="checkbox" type="checkbox" checked={duckOutput} onChange={(e) => void onConfigChanged({ duck_output_while_recording: e.target.checked })}/>
            {t("Приглушать звук")}
          </label>
          <HintIcon text={t("На время записи убавить общую громкость и вернуть её после. Нужно, если пишете с колонок: звук из них попадает в микрофон.")}/>
        </span>
        {/* The whole test is a single icon button, and the same button
            shows the result: a tick or a red mark instead of words beside
            it. That way the row takes the same width in every state —
            whereas the button appearing, and then the status, used to push
            the cell onto a second line and change the card's height right
            under the cursor. The result text has not gone anywhere: it is in
            the button's hint and in the hidden screen-reader line. */}
        <Hint text={duckTestLabel}>
          <button
            className="btn btn--ghost behavior-row__duck-button"
            type="button"
            data-state={duckTest}
            data-visible={duckOutput ? "true" : "false"}
            disabled={!duckOutput || duckTest === "running"}
            tabIndex={duckOutput ? undefined : -1}
            aria-hidden={duckOutput ? undefined : true}
            onClick={() => void testOutputDuck()}
            aria-label={duckTestLabel}
          >
            <Icon name={duckTest === "done" ? "check" : duckTest === "error" ? "x" : "test"} size={12}/>
          </button>
        </Hint>
        <span className="sr-only" role="status">{duckTest === "idle" ? "" : duckTestLabel}</span>
      </div>
    </div>
  );
}
