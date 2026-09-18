import { useEffect, useRef, useState, type CSSProperties } from "react";
import { Segmented } from "../../components/Shell";
import { Icon } from "../../components/Icon";
import { Hint } from "../../components/Hint";
import { CustomSelect } from "../../components/CustomSelect";
import { NumberField } from "../../components/NumberField";
import { t } from "../../i18n";
import type { ConfigResult } from "../../bridge/types";
import { ACCENT_PRESETS, applyAccent, resolveAccent } from "../../accent";
import { modelUnloadMinutes, modelUnloadOptions } from "../modelUnloadSettings";
import { isTelemetryEnabled } from "../telemetrySettings";
import { OverlaySettings } from "../OverlaySettings";
import { HintIcon, SetLabel, type ConfigChanged } from "./controls";

// Changing the device reloads the model on the Rust side — that is the only
// moment whisper.cpp applies use_gpu. While the reload runs, the usual
// model-loading / model-ready events arrive.
function DevicePicker({ device, cpuOnly, onConfigChanged }: { device?: string; cpuOnly?: boolean; onConfigChanged: ConfigChanged }) {
  // Anything but an explicit "cpu" is GPU: Rust reckons the same way
  // (`resolve_device`), including the legacy value "cuda".
  const current = cpuOnly ? "cpu" : (device === "cpu" ? "cpu" : "gpu");
  // A choice between two mutually exclusive values is a switch, not a list:
  // both options are visible at once and there is no point opening a menu for
  // them.
  //
  // For a CPU-only model the switch is disabled and the "why" moved into a hint
  // on the switch itself: people ask about it exactly when it is disabled, and
  // they ask it. A permanent caption beside it cost a line of text and a column
  // of width in a row that was already short of width.
  const picker = (
    <Segmented
      value={current}
      disabled={cpuOnly}
      options={[
        { value: "gpu", label: "GPU", icon: "gpu" },
        { value: "cpu", label: "CPU", icon: "cpu" },
      ]}
      onChange={(next) => { if (!cpuOnly) void onConfigChanged({ device: next as "gpu" | "cpu" }); }}
    />
  );
  return (
    <div className="device-picker">
      {cpuOnly ? <Hint text={t("Модель работает только на CPU")}>{picker}</Hint> : picker}
    </div>
  );
}

// Duplicated in src-tauri/src/history.rs (RetentionPolicy::default).
const DEFAULT_HISTORY_RETENTION_DAYS = 30;
const DEFAULT_HISTORY_MAX_ENTRIES = 1000;
const MAX_HISTORY_RETENTION_DAYS = 3650;

function HistoryRetentionControl({ days, maxEntries, onConfigChanged }: { days: number; maxEntries: number; onConfigChanged: ConfigChanged }) {
  const [draftDays, setDraftDays] = useState(String(days));
  const [draftEntries, setDraftEntries] = useState(String(maxEntries));

  useEffect(() => { setDraftDays(String(days)); }, [days]);
  useEffect(() => { setDraftEntries(String(maxEntries)); }, [maxEntries]);

  // Matches the check in Rust: a value outside the range is rolled back to the
  // default there, so it must never reach the config.
  function clamp(raw: string, fallback: number, max: number) {
    const parsed = Number(raw);
    if (!Number.isFinite(parsed)) return fallback;
    return Math.max(0, Math.min(max, Math.round(parsed)));
  }

  async function saveDays(raw = draftDays) {
    const value = clamp(raw, DEFAULT_HISTORY_RETENTION_DAYS, MAX_HISTORY_RETENTION_DAYS);
    setDraftDays(String(value));
    await onConfigChanged({ history_retention_days: value });
  }

  async function saveEntries(raw = draftEntries) {
    const value = clamp(raw, DEFAULT_HISTORY_MAX_ENTRIES, 1_000_000);
    setDraftEntries(String(value));
    await onConfigChanged({ history_max_entries: value });
  }

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <NumberField className="mono" min={0} max={MAX_HISTORY_RETENTION_DAYS} value={draftDays} onValueChange={setDraftDays} onStepCommit={(next) => void saveDays(next)} onBlur={() => void saveDays()} onKeyDown={(e) => { if (e.key === "Enter") void saveDays(); }} style={{ width: 66, height: "var(--control-h)", flex: "0 0 auto" }}/>
        <span style={{ color: "var(--ink-mute)", font: "500 12px/1 var(--font-sans)" }}>{t("дн")}</span>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <NumberField className="mono" min={0} value={draftEntries} onValueChange={setDraftEntries} onStepCommit={(next) => void saveEntries(next)} onBlur={() => void saveEntries()} onKeyDown={(e) => { if (e.key === "Enter") void saveEntries(); }} style={{ width: 78, height: "var(--control-h)", flex: "0 0 auto" }}/>
        <span style={{ color: "var(--ink-mute)", font: "500 12px/1 var(--font-sans)" }}>{t("записей")}</span>
      </div>
    </div>
  );
}

function TypingSpeedControl({ value, onConfigChanged }: { value?: number; onConfigChanged: ConfigChanged }) {
  const [draft, setDraft] = useState(String(value ?? 240));

  useEffect(() => {
    setDraft(String(value ?? 240));
  }, [value]);

  async function save(nextValue = draft) {
    const parsed = Number(nextValue);
    const normalized = Math.max(60, Math.min(900, Number.isFinite(parsed) ? Math.round(parsed) : 240));
    setDraft(String(normalized));
    await onConfigChanged({ typing_speed_cpm: normalized });
  }

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <NumberField className="mono" min={60} max={900} step={10} value={draft} onValueChange={setDraft} onStepCommit={(next) => void save(next)} onBlur={() => void save()} onKeyDown={(e) => { if (e.key === "Enter") void save(); }} style={{ width: 78, height: "var(--control-h)", flex: "0 0 auto" }}/>
        <span style={{ font: "500 12px/1 var(--font-sans)", color: "var(--ink-dim)", whiteSpace: "nowrap" }}>{t("симв/мин")}</span>
      </div>
    </div>
  );
}

// After how much idling the model leaves RAM. The value itself lives in
// ./modelUnloadSettings because Rust reads the very same one.
function ModelUnloadControl({ value, onConfigChanged }: { value?: number; onConfigChanged: ConfigChanged }) {
  const current = modelUnloadMinutes(value);
  const options = modelUnloadOptions(current).map((minutes) => ({
    value: minutes,
    // «Никогда» is not a duration, hence not "0 min" either.
    label: minutes === 0 ? t("Никогда") : t("{p0} мин", { p0: minutes }),
  }));
  return (
    <CustomSelect
      className="custom-select--model-unload"
      value={current}
      options={options}
      onChange={(next) => void onConfigChanged({ model_unload_after_minutes: next })}
    />
  );
}

// The interface colour. It used to be a fourth overlay setting named «Акцент
// приложения» and the overlay could follow it; both were confusing — one
// control coloured two unrelated things. Here it colours the app, and the
// overlay keeps its own palette.
//
// Free-form: the four presets are shortcuts, and everything else comes from the
// system colour picker. Dragging in that picker fires a change per frame, so
// the colour is applied to the CSS variables immediately and written to the
// config once the dragging settles.
const ACCENT_COMMIT_DELAY = 250;

function InterfaceColorPicker({ value, onConfigChanged }: { value?: string; onConfigChanged: ConfigChanged }) {
  const accent = resolveAccent(value);
  const presets = ACCENT_PRESETS();
  const [saving, setSaving] = useState(false);
  const savedAccent = useRef(accent);
  savedAccent.current = accent;
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => {
    if (timer.current) {
      clearTimeout(timer.current);
      applyAccent(savedAccent.current);
    }
  }, []);

  async function commit(color: string) {
    timer.current = null;
    setSaving(true);
    try {
      const saved = await onConfigChanged({ ui_accent: color });
      applyAccent(saved ? resolveAccent(saved.ui_accent) : savedAccent.current);
    } catch {
      applyAccent(savedAccent.current);
    } finally {
      setSaving(false);
    }
  }

  function pick(color: string, immediate: boolean) {
    applyAccent(color);
    if (timer.current) clearTimeout(timer.current);
    if (immediate) void commit(color);
    else timer.current = setTimeout(() => void commit(color), ACCENT_COMMIT_DELAY);
  }

  return (
    <div className="accent-picker" role="group" aria-label={t("Цвет интерфейса")}>
      {presets.map((preset) => (
        <Hint key={preset.value} text={preset.label}>
          <button
            type="button"
            className="overlay-swatch overlay-swatch--accent"
            aria-label={preset.label}
            aria-pressed={preset.value === accent}
            disabled={saving}
            style={{ "--swatch": preset.value } as CSSProperties}
            onClick={() => pick(preset.value, true)}
          />
        </Hint>
      ))}
      <Hint text={t("Свой цвет")}>
        <span className="accent-picker__custom" data-active={!presets.some((preset) => preset.value === accent)}>
          <input
            type="color"
            aria-label={t("Свой цвет")}
            value={accent}
            disabled={saving}
            onChange={(event) => pick(event.target.value.toLowerCase(), false)}
          />
        </span>
      </Hint>
    </div>
  );
}

export function AdvancedSection({ config, portable, cpuOnly, onConfigChanged }: {
  config: ConfigResult | null;
  portable?: boolean;
  cpuOnly?: boolean;
  onConfigChanged: ConfigChanged;
}) {
  const autoPaste = config?.auto_paste ?? true;
  const autoStart = config?.auto_start ?? false;

  return (
    <details className="card card--rows advanced">
      <summary>
        <Icon name="chev-down" size={13}/>
        {t("Дополнительно")}
      </summary>

      <div className="advanced__main-row">
        <div className="set-cell">
          <SetLabel title={t("Устройство обработки")}/>
          <DevicePicker device={config?.device} cpuOnly={cpuOnly} onConfigChanged={onConfigChanged}/>
        </div>
        <div className="vrule"/>
        <div className="set-cell advanced__history-cell">
          <SetLabel title={t("Хранить историю")} hint={t("Записи старше указанного срока и всё, что не влезло в лимит, удаляются при открытии страницы истории. 0 — без ограничения.")}/>
          <HistoryRetentionControl
            days={config?.history_retention_days ?? DEFAULT_HISTORY_RETENTION_DAYS}
            maxEntries={config?.history_max_entries ?? DEFAULT_HISTORY_MAX_ENTRIES}
            onConfigChanged={onConfigChanged}
          />
        </div>
        <div className="vrule"/>
        <div className="set-cell">
          <SetLabel title={t("Скорость набора")} hint={t("Скорость ручного набора. В статистике используется формула: символы / скорость набора.")}/>
          <TypingSpeedControl value={config?.typing_speed_cpm} onConfigChanged={onConfigChanged}/>
        </div>
        {/* The row's fourth setting. On a narrow card — with the sidebar open
            at the minimum window width — it moves to a second row and the
            separator before it is hidden: that is handled by a container
            query in styles.css which measures the card, not the window. */}
        <div className="vrule advanced__unload-rule"/>
        <div className="set-cell advanced__unload-cell">
          <SetLabel title={t("Выгружать модель")} hint={t("Через сколько минут без диктовки освобождать оперативную память. Модель вернётся в неё сама — в начале следующей записи, пока вы говорите.")}/>
          <ModelUnloadControl value={config?.model_unload_after_minutes} onConfigChanged={onConfigChanged}/>
        </div>
      </div>

      {/* Clarifications to auto-paste: they work only when it is on, and when
          disabled they look dimmed — otherwise a checkbox that does nothing
          reads as broken. */}
      <div className="advanced__paste-row">
        <label className="checkbox-row" style={{ color: autoPaste ? "var(--ink-mute)" : "var(--ink-faint)" }}>
          <input className="checkbox" type="checkbox" disabled={!autoPaste} checked={config?.paste_trailing_space ?? false} onChange={(e) => void onConfigChanged({ paste_trailing_space: e.target.checked })}/>
          {t("Пробел в конце")}
        </label>
        <span className="label-with-hint">
          <label className="checkbox-row" style={{ color: autoPaste ? "var(--ink-mute)" : "var(--ink-faint)" }}>
            <input className="checkbox" type="checkbox" disabled={!autoPaste} checked={config?.paste_auto_submit ?? false} onChange={(e) => void onConfigChanged({ paste_auto_submit: e.target.checked })}/>
            {t("Enter после вставки")}
          </label>
          <HintIcon text={t("Нажать Enter сразу после вставки — отправит сообщение в чате или запустит поиск.")}/>
        </span>
        <div className="set-cell advanced__appearance-cell">
          <SetLabel title={t("Цвет интерфейса")} hint={t("Цвет кнопок, выделения и активных элементов во всём приложении. Цвет оверлея настраивается отдельно, в разделе «Оверлей».")}/>
          <InterfaceColorPicker value={config?.ui_accent} onConfigChanged={onConfigChanged}/>
        </div>
      </div>

      {/* Autostart is set once per installation — exactly the case this block
          is collapsed for. Silence trimming is no longer here: vad.rs refuses
          to trim by itself when no speech is found or the saving is under a
          second, so there was nothing for the switch to fix. The trim_silence
          key in config.json is still read — as a debug option.
          The telemetry consent sits here too: it is the same kind of "set once
          and forget" switch, and a separate subsection for a single row was
          heavier than the row itself. */}
      <div className="advanced__autostart-row">
        {/* A portable copy does not register autostart: the entry it would
            write points at a folder that travels on a flash drive, and
            rewriting it would take the installed copy's autostart away.
            Hence a disabled checkbox with the reason on it rather than a
            working-looking one that saves the value and does nothing. */}
        <span className="label-with-hint">
          <label className="checkbox-row" style={{ color: portable ? "var(--ink-faint)" : undefined }}>
            <input className="checkbox" type="checkbox" disabled={portable} checked={autoStart && !portable} onChange={(e) => void onConfigChanged({ auto_start: e.target.checked })}/>
            {t("Запускать вместе с системой")}
          </label>
          <HintIcon text={portable
            ? t("Недоступно в портативной версии: запись автозапуска указывала бы на папку, которая ездит вместе с приложением, и перебила бы автозапуск установленной копии.")
            : t("Приложение запускается в фоне при входе в систему, горячая клавиша становится доступна сразу.")}/>
        </span>
        <span className="label-with-hint">
          <label className="checkbox-row">
            <input
              className="checkbox"
              type="checkbox"
              checked={isTelemetryEnabled(config?.telemetry_enabled)}
              onChange={(e) => void onConfigChanged({ telemetry_enabled: e.target.checked })}
            />
            {t("Разрешить обезличенную телеметрию")}
          </label>
          <HintIcon text={t("Собираются обезличенные события использования и технические сведения: режим обработки, длительность аудио и обработки, оценка сэкономленного времени, ОС, версия приложения, архитектура и сведения о сессии.")}/>
        </span>
      </div>
      <OverlaySettings config={config} onConfigChanged={onConfigChanged}/>
    </details>
  );
}
