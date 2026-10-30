import { useState } from "react";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { CustomSelect } from "../../components/CustomSelect";
import { Icon } from "../../components/Icon";
import { Hint } from "../../components/Hint";
import { SLIDER_RELEASE_KEYS } from "../../components/sliderKeys";
import { HintIcon, type ConfigChanged } from "./controls";
import { t } from "../../i18n";
import type { ConfigResult, SoundCue, SoundCueSetting, SoundTheme } from "../../bridge/types";
import {
  CUE_RANGE_DB, DEFAULT_SOUND_THEME, DEFAULT_SOUND_VOLUME, OVERALL_RANGE_DB,
  amplitudeAt, cuePatch, cueSetting, positionOf, previewCue,
} from "./soundCueSettings";

// A pseudo-theme for the «Выключено» option. To the user "are the sounds on"
// and "which sounds" are one choice, so there is one control.
const SOUND_OFF = "off";
// A pseudo-source that opens the file picker instead of being saved.
const PICK_FILE = "pick";
// A cue's sound from the overall theme, whichever it is; the theme names in
// the same list pin a cue to that theme instead.
const FOLLOW_THEME = "follow";

type SourceChoice = typeof FOLLOW_THEME | SoundTheme | "custom" | "mute" | typeof PICK_FILE;

function sourceChoice(setting: SoundCueSetting): SourceChoice {
  return setting.source === "theme" ? setting.theme ?? FOLLOW_THEME : setting.source;
}

function choiceChange(choice: Exclude<SourceChoice, typeof PICK_FILE>): Partial<SoundCueSetting> {
  if (choice === FOLLOW_THEME) return { source: "theme", theme: null };
  if (choice === "custom" || choice === "mute") return { source: choice };
  return { source: "theme", theme: choice };
}

const THEME_OPTIONS = () => ([
  { label: t("Дерево"), value: "wood" as const },
  { label: t("Аккорд"), value: "chord" as const },
  { label: t("Сонар"), value: "sonar" as const },
  { label: t("Классический"), value: "classic" as const },
]);

const CUE_ROWS = () => ([
  { cue: "start" as const, label: t("Начало записи") },
  { cue: "stop" as const, label: t("Окончание записи"), hint: t("Звучит только в режиме «Переключение»: при удержании конец записи отмечает сама отпущенная клавиша.") },
  { cue: "done" as const, label: t("Вставка текста") },
  { cue: "error" as const, label: t("Ошибка") },
]);

function soundState(config: ConfigResult | null) {
  return {
    enabled: config?.sound_feedback ?? true,
    theme: config?.sound_theme ?? DEFAULT_SOUND_THEME,
    overall: config?.sound_volume ?? DEFAULT_SOUND_VOLUME,
  };
}

/** A volume slider that saves on release: dragging only moves the thumb, and
 *  letting go saves the value and plays it. */
function VolumeSlider({ position, label, disabled, onCommit }: { position: number; label: string; disabled?: boolean; onCommit: (position: number) => Promise<unknown> }) {
  const [draft, setDraft] = useState<number | null>(null);
  // The draft is held until the save lands, so the thumb does not jump back to
  // the previous value for the length of the round trip.
  const commit = () => {
    if (draft === null) return;
    void onCommit(draft).finally(() => setDraft(null));
  };
  return (
    <input
      type="range"
      className="volume-slider"
      min={0}
      max={100}
      step={1}
      value={draft ?? position}
      aria-label={label}
      disabled={disabled}
      onChange={(event) => setDraft(Number(event.target.value))}
      onPointerUp={commit}
      onKeyUp={(event) => { if (SLIDER_RELEASE_KEYS.includes(event.key)) commit(); }}
      onBlur={commit}
    />
  );
}

function PlayButton({ label, disabled, onClick }: { label: string; disabled?: boolean; onClick: () => void }) {
  return (
    <Hint text={label}>
      <button className="btn btn--ghost btn--sm btn--icon" type="button" disabled={disabled} aria-label={label} onClick={onClick}>
        <Icon name="play" size={11}/>
      </button>
    </Hint>
  );
}

// Dictation sound cues: recording start, end, text insertion, error. One line
// sets the theme and the overall volume; the panel under the row tunes each cue.
// Choosing a theme or letting go of the slider plays the start cue, so the line
// needs no play button of its own; the other cues are heard in the panel.
export function SoundFeedbackControl({ config, onConfigChanged, tuning, onToggleTuning, panelId }: {
  config: ConfigResult | null;
  onConfigChanged: ConfigChanged;
  tuning: boolean;
  onToggleTuning: () => void;
  panelId: string;
}) {
  const { enabled, theme, overall } = soundState(config);
  const start = cueSetting(config, "start");

  // «Выключено» touches neither the theme nor the volumes: bringing the cues
  // back restores exactly what the user had.
  function select(next: SoundTheme | typeof SOUND_OFF) {
    if (next === SOUND_OFF) {
      void onConfigChanged({ sound_feedback: false });
      return;
    }
    void onConfigChanged({ sound_feedback: true, sound_theme: next });
    // The theme's own start sound, even when that cue is muted or replaced by
    // a file: this is what the choice being made sounds like.
    previewCue("start", next, { ...start, source: "theme", theme: null }, overall);
  }

  const tuneLabel = t("Настроить каждый сигнал");
  return (
    <div className="sound-feedback-control">
      <span className="label-with-hint">
        <span className="sound-feedback-control__label">{t("Звуковые сигналы")}</span>
        <HintIcon text={t("Сигналы отмечают начало записи, завершение, вставку текста и ошибку.")}/>
      </span>
      <CustomSelect
        className="custom-select--sound-theme"
        value={enabled ? theme : SOUND_OFF}
        options={[{ label: t("Выключено"), value: SOUND_OFF }, ...THEME_OPTIONS()]}
        onChange={select}
      />
      <VolumeSlider
        position={positionOf(overall, OVERALL_RANGE_DB)}
        label={t("Громкость сигналов")}
        disabled={!enabled}
        onCommit={(position) => {
          const volume = amplitudeAt(position, OVERALL_RANGE_DB);
          previewCue("start", theme, start, volume);
          return onConfigChanged({ sound_volume: volume });
        }}
      />
      <Hint text={tuneLabel}>
        <button
          className="btn btn--ghost btn--sm btn--icon sound-feedback-control__tune"
          type="button"
          disabled={!enabled}
          aria-label={tuneLabel}
          aria-expanded={enabled && tuning}
          aria-controls={panelId}
          onClick={onToggleTuning}
        >
          <Icon name="sliders" size={13}/>
        </button>
      </Hint>
    </div>
  );
}

/** Per-cue sound and volume, under the behaviour row. */
export function SoundCuePanel({ id, config, onConfigChanged }: { id: string; config: ConfigResult | null; onConfigChanged: ConfigChanged }) {
  const { theme, overall } = soundState(config);
  const [importing, setImporting] = useState<SoundCue | null>(null);
  const [errors, setErrors] = useState<Partial<Record<SoundCue, string>>>({});

  async function pickFile(cue: SoundCue) {
    setImporting(cue);
    setErrors((current) => ({ ...current, [cue]: undefined }));
    try {
      const fileName = await tauriInvoke<string | null>("import_sound_cue", { cue });
      if (fileName === null) return;
      const change = { source: "custom" as const, file_name: fileName };
      await onConfigChanged(cuePatch(cue, change));
      previewCue(cue, theme, { ...cueSetting(config, cue), ...change }, overall);
    } catch (error) {
      setErrors((current) => ({ ...current, [cue]: error instanceof Error ? error.message : String(error) }));
    } finally {
      setImporting(null);
    }
  }

  function choose(cue: SoundCue, value: SourceChoice) {
    if (value === PICK_FILE) {
      void pickFile(cue);
      return;
    }
    const change = choiceChange(value);
    void onConfigChanged(cuePatch(cue, change));
    previewCue(cue, theme, { ...cueSetting(config, cue), ...change }, overall);
  }

  return (
    <div id={id} className="behavior-row__sound-cues" role="group" aria-label={t("Звуковые сигналы")}>
      {CUE_ROWS().map(({ cue, label, hint }) => {
        const setting = cueSetting(config, cue);
        const muted = setting.source === "mute";
        // An imported file stays on offer after switching away from it, so
        // going back does not mean picking it again.
        const custom = setting.file_name || setting.source === "custom"
          ? [{ label: setting.file_name || t("Свой звук"), value: "custom" as const }]
          : [];
        const options: Array<{ label: string; value: SourceChoice }> = [
          { label: t("Из общего набора"), value: FOLLOW_THEME },
          ...THEME_OPTIONS(),
          ...custom,
          { label: t("Выбрать файл…"), value: PICK_FILE },
          { label: t("Без звука"), value: "mute" },
        ];
        return (
          <div className="sound-cues__row" key={cue}>
            <span className="label-with-hint sound-cues__label">
              {label}
              {hint && <HintIcon text={hint}/>}
            </span>
            <CustomSelect
              className="custom-select--sound-source"
              value={sourceChoice(setting)}
              disabled={importing === cue}
              options={options}
              onChange={(value) => choose(cue, value)}
            />
            <VolumeSlider
              position={positionOf(setting.volume, CUE_RANGE_DB)}
              label={t("Громкость: {p0}", { p0: label })}
              disabled={muted}
              onCommit={(position) => {
                const volume = amplitudeAt(position, CUE_RANGE_DB);
                previewCue(cue, theme, { ...setting, volume }, overall);
                return onConfigChanged(cuePatch(cue, { volume }));
              }}
            />
            <PlayButton label={t("Прослушать: {p0}", { p0: label })} disabled={muted} onClick={() => previewCue(cue, theme, setting, overall)}/>
            {errors[cue] && <span className="sound-cues__error" role="alert">{errors[cue]}</span>}
          </div>
        );
      })}
    </div>
  );
}
