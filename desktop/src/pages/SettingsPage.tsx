import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke, on } from "../bridge";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { PageHeader, Segmented } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Hint } from "../components/Hint";
import { CustomSelect, type SelectOption } from "../components/CustomSelect";
import { NumberField } from "../components/NumberField";
import type { ConfigResult, MicrophoneResult, ModelInfo } from "../bridge/types";
import { getLocale, isLocale, LOCALE_LABELS, LOCALES, setLocale, t, type Locale } from "../i18n";
import { DEFAULT_HOTKEY, normalizeHotkeyKey } from "../hotkey";
import { fallbackLanguage, fallbackModels, speechLanguages } from "./modelCatalog";
import { isTelemetryEnabled } from "./telemetrySettings";
import { modelUnloadMinutes, modelUnloadOptions } from "./modelUnloadSettings";

type Props = {
  config: ConfigResult | null;
  microphones: MicrophoneResult[];
  models: ModelInfo[];
  onConfigChanged: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null>;
};

function hotkeyLabel(hotkey: string | undefined, fallback: string) {
  return (hotkey || fallback).split("+").map((part) => part.trim()).filter(Boolean);
}

function HotkeyDisplay({ hotkey, fallback, onConfigChanged }: {
  hotkey?: string;
  fallback: string;
  onConfigChanged: Props["onConfigChanged"];
}) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(hotkey || fallback);
  const [recording, setRecording] = useState(false);
  const [pressedKeys, setPressedKeys] = useState<Set<string>>(new Set());
  const pressedRef = useRef<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

  // Sync local value when the persisted hotkey changes (e.g. config reload).
  useEffect(() => {
    if (!recording) setValue(hotkey || fallback);
  }, [hotkey, fallback, recording]);

  // macOS-reserved combos we refuse to bind locally. The sidecar `parse_hotkey`
  // would still accept these technically, but binding them bricks the user out of
  // the app. Anything else is delegated to the sidecar validator.
  const RESERVED = new Set([
    "cmd+q",
    "cmd+w",
    "cmd+h",
    "cmd+m",
    "cmd+space",
    "ctrl+cmd+q",
  ]);

  const MODIFIERS = new Set(["ctrl", "alt", "shift", "cmd"]);


  function formatCombo(set: Set<string>): string {
    const MOD_ORDER = ["ctrl", "alt", "shift", "cmd"];
    const mods: string[] = [];
    const nonMods: string[] = [];
    for (const k of set) {
      if (MODIFIERS.has(k)) mods.push(k);
      else nonMods.push(k);
    }
    mods.sort((a, b) => MOD_ORDER.indexOf(a) - MOD_ORDER.indexOf(b));
    return [...mods, ...nonMods].join("+");
  }

  function cancelRecording() {
    pressedRef.current = new Set();
    setPressedKeys(new Set());
    setRecording(false);
  }

  async function commit(combo: string) {
    setError(null);
    if (!combo) return;
    if (RESERVED.has(combo)) {
      const pretty = combo.split("+").join(" + ");
      setError(t("Комбинация {p0} зарезервирована системой. Выберите другую.", { p0: pretty }));
      cancelRecording();
      return;
    }
    // Update the text input immediately so the user sees what we caught.
    setValue(combo);
    // Validate via Rust (Result<(), String>) — throws on error.
    try {
      await tauriInvoke("validate_hotkey", { hotkey: combo });
    } catch (e) {
      setError(String(e));
      cancelRecording();
      return;
    }
    // Apply through the existing onConfigChanged callback — it already does
    // save_config + state update via the bridge. We additionally call
    // set_hotkey to register the global shortcut at runtime.
    try {
      const oldHotkey = hotkey ?? "";
      await tauriInvoke("set_hotkey", { hotkey: combo, oldHotkey });
      await onConfigChanged({ hotkey: combo });
      cancelRecording();
      setEditing(false);
    } catch (e) {
      setError(String(e));
      cancelRecording();
    }
  }

  function startRecording() {
    setError(null);
    pressedRef.current = new Set();
    setPressedKeys(new Set());
    setRecording(true);
  }

  // Global key listeners are only attached while `recording` is true. We rely
  // on the in-window capture (the Tauri webview only sees the focused window)
  // — a true global capture would require macOS Accessibility TCC and is out
  // of scope here.
  useEffect(() => {
    if (!recording) return;
    function hasNonModifier(s: Set<string>): boolean {
      for (const k of s) if (!MODIFIERS.has(k)) return true;
      return false;
    }
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
        e.preventDefault();
        e.stopPropagation();
        cancelRecording();
        return;
      }
      const token = normalizeHotkeyKey(e);
      if (!token) return;
      // Suppress browser/shortcut interference (Cmd+Space → Spotlight, etc.)
      e.preventDefault();
      e.stopPropagation();
      // Ignore OS key auto-repeat: the combo is captured once, on the initial
      // press of the non-modifier key, using whatever modifiers are held.
      if (e.repeat) return;
      if (e.ctrlKey) pressedRef.current.add("ctrl");
      if (e.altKey) pressedRef.current.add("alt");
      if (e.shiftKey) pressedRef.current.add("shift");
      if (e.metaKey) pressedRef.current.add("cmd");
      pressedRef.current.add(token);
      setPressedKeys(new Set(pressedRef.current));
      // A non-modifier is the "main" key — snapshot the current chord and
      // commit immediately. This avoids races between an idle timer and
      // keyup-driven mutations that caused combos to capture unreliably.
      if (hasNonModifier(pressedRef.current)) {
        const combo = formatCombo(pressedRef.current);
        pressedRef.current = new Set();
        setRecording(false);
        void commit(combo);
      }
    }
    function onKeyUp(e: KeyboardEvent) {
      const token = normalizeHotkeyKey(e);
      if (!token) return;
      // Don't stopPropagation on keyup — let release events through.
      if (pressedRef.current.has(token)) {
        pressedRef.current.delete(token);
        setPressedKeys(new Set(pressedRef.current));
      }
    }
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
    };
    // We intentionally re-attach only when `recording` flips. The handlers read
    // stable refs and setters, so stale closure is not an issue here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recording]);

  if (editing) {
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <div className="hotkey-edit" style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
          <input
            className="field mono"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void commit(value);
              if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) { cancelRecording(); setEditing(false); }
            }}
            autoFocus
            disabled={recording}
            placeholder={recording ? t("Нажмите комбинацию…") : undefined}
            style={{ height: "var(--control-h)", maxWidth: 260 }}
          />
          {!recording && (
            <>
              <button className="btn btn--primary" type="button" onClick={() => void commit(value)}>
                <Icon name="check" size={12}/>{t("Применить")} </button>
              <button className="btn btn--ghost" type="button" onClick={startRecording} aria-pressed={recording}>
                <Icon name="key" size={12}/>{t("Записать")} </button>
              <button className="btn btn--ghost" type="button" onClick={() => setEditing(false)}>{t("Отмена")}</button>
            </>
          )}
          {recording && (
            <button className="btn btn--ghost hotkey-cancel-btn" type="button" onClick={cancelRecording} aria-label={t("Отменить запись")}>
               {t("Esc — отмена")} </button>
          )}
        </div>
        {recording && (
          <div className="hotkey-record-chip" role="status" aria-live="polite">
            <span className="hotkey-record-chip__hint">{t("Нажмите комбинацию")}</span>
            <span className="hotkey-record-chip__keys">
              {pressedKeys.size === 0 ? (
                <span className="kbd hotkey-record-chip__placeholder">…</span>
              ) : (
                (() => {
                  const combo = formatCombo(pressedKeys);
                  const parts = combo.split("+");
                  return parts.map((part, i, arr) => (
                    <span key={`${part}-${i}`} className="hotkey-record-chip__key" style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                      <span className="kbd">{part}</span>
                      {i < arr.length - 1 && <span style={{ color: "var(--text-mute)" }}>+</span>}
                    </span>
                  ));
                })()
              )}
            </span>
          </div>
        )}
        {error && <div style={{ color: "var(--err)", font: "500 11px/1.4 var(--font-sans)" }}>{error}</div>}
      </div>
    );
  }

  return (
    <div className="hotkey-display">
      <div className="hotkey-display__keys">
        {hotkeyLabel(hotkey, fallback).map((key, i, arr) => <span key={`${key}-${i}`} style={{ display: "inline-flex", alignItems: "center", gap: 6 }}><span className="kbd">{key}</span>{i < arr.length - 1 && <span style={{ color: "var(--text-mute)" }}>+</span>}</span>)}
      </div>
      <button
        className="btn btn--ghost hotkey-display__edit"
        type="button"
        title={t("Изменить")}
        aria-label={t("Изменить")}
        onClick={() => { setValue(hotkey || fallback); setEditing(true); }}
      ><Icon name="pencil" size={13}/></button>
    </div>
  );
}

function RecordingModeSegmented({ value, onConfigChanged }: { value: string; onConfigChanged: Props["onConfigChanged"] }) {
  const normalized = value === "push_to_talk" ? "push_to_talk" : "toggle";
  return (
    <div className="capture-row__recording-mode">
      <Segmented
        value={normalized}
        options={[
          { value: "toggle", label: t("Переключать"), icon: "refresh" },
          { value: "push_to_talk", label: t("Удерживать"), icon: "mic" },
        ]}
        onChange={(next) => void onConfigChanged({ recording_mode: next as "toggle" | "push_to_talk" })}
      />
    </div>
  );
}

function HintIcon({ text }: { text: string }) {
  return <Hint text={text}/>;
}

function SetLabel({ title, hint }: { title: string; hint?: string }) {
  return (
    <span className="set-label">
      {title}
      {hint && <HintIcon text={hint}/>}
    </span>
  );
}

// The UI language. Deliberately separate from LanguagePicker: that one is about
// the speech language, and confusing the two is expensive — picking «English» in
// the hope of switching the interface breaks Russian dictation.
function UiLanguagePicker({ value, onConfigChanged }: { value?: Locale; onConfigChanged: (patch: Partial<ConfigResult>) => Promise<unknown> }) {
  const current = value ?? getLocale();
  // The wrapper exists for the height: Segmented is styled inline, and its
  // buttons can only be reached through a class from outside.
  return (
    <div className="lang-row__ui-language">
      <Segmented
        value={current}
        options={LOCALES.map((locale) => ({ value: locale, label: LOCALE_LABELS[locale] }))}
        onChange={(next) => {
          if (!isLocale(next)) return;
          // Applied at once without waiting for the answer: saving may take
          // hundreds of milliseconds while a switch must respond instantly.
          setLocale(next);
          void onConfigChanged({ ui_language: next });
        }}
      />
    </div>
  );
}

function LanguagePicker({ language, model, models, onConfigChanged }: { language?: string; model?: ModelInfo; models: ModelInfo[]; onConfigChanged: Props["onConfigChanged"] }) {
  // A model with a closed language list decides for that list itself: both
  // English-only Whisper builds and GigaAM produce garbage rather than an error
  // on a foreign language.
  const correction = fallbackLanguage(model, language);
  useEffect(() => {
    if (correction) void onConfigChanged({ language: correction });
  }, [correction, onConfigChanged]);
  // The same list as in the catalog: it would be odd to be able to download a
  // German model yet unable to say you are dictating in German. There are
  // deliberately no flags — a language is not a country, and English and Arabic
  // have a dozen each.
  const options = useMemo<Array<SelectOption<string>>>(
    () => [
      { value: "auto", label: t("Авто"), icon: "globe" },
      ...speechLanguages(model, models).map((item) => ({
        value: item.code,
        label: item.name,
        meta: item.code.toUpperCase(),
      })),
    ],
    [model, models],
  );
  const value = correction ?? language ?? "ru";
  return <CustomSelect className="custom-select--language" value={value} options={options} searchable inlineMeta onChange={(next) => void onConfigChanged({ language: next })}/>;
}

// Changing the device reloads the model on the Rust side — that is the only
// moment whisper.cpp applies use_gpu. While the reload runs, the usual
// model-loading / model-ready events arrive.
function DevicePicker({ device, cpuOnly, onConfigChanged }: { device?: string; cpuOnly?: boolean; onConfigChanged: Props["onConfigChanged"] }) {
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
function SoundFeedbackControl({ enabled, volume, onConfigChanged }: { enabled: boolean; volume: number; onConfigChanged: Props["onConfigChanged"] }) {
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

// After how much idling the model leaves RAM. The value itself lives in
// ./modelUnloadSettings because Rust reads the very same one.
function ModelUnloadControl({ value, onConfigChanged }: { value?: number; onConfigChanged: Props["onConfigChanged"] }) {
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

// Duplicated in src-tauri/src/history.rs (RetentionPolicy::default).
const DEFAULT_HISTORY_RETENTION_DAYS = 30;
const DEFAULT_HISTORY_MAX_ENTRIES = 1000;
const MAX_HISTORY_RETENTION_DAYS = 3650;

function HistoryRetentionControl({ days, maxEntries, onConfigChanged }: { days: number; maxEntries: number; onConfigChanged: Props["onConfigChanged"] }) {
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

const MIC_SEGMENTS = 24;

function MicMeter({ level, peak, active }: { level: number; peak: number; active: boolean }) {
  // Left-to-right VU meter: the lit length tracks the current loudness, so the
  // test tells you how well (and how loudly) you're heard — not just that the
  // mic is alive. Colour zones flag quiet / good / too-hot, and a peak-hold
  // marker keeps the recent maximum visible. This intentionally differs from
  // the recording overlay's rolling waveform.
  const clamped = Math.min(1, Math.max(0, level));
  const litCount = Math.round(clamped * MIC_SEGMENTS);
  const peakIndex = peak > 0 ? Math.min(MIC_SEGMENTS - 1, Math.max(0, Math.ceil(peak * MIC_SEGMENTS) - 1)) : -1;
  return (
    <div
      className={`mic-meter${active ? " mic-meter--active" : ""}`}
      role="meter"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(clamped * 100)}
    >
      {Array.from({ length: MIC_SEGMENTS }, (_, index) => {
        const fraction = (index + 1) / MIC_SEGMENTS;
        const zone = fraction > 0.88 ? "hot" : fraction > 0.7 ? "warn" : "ok";
        const lit = index < litCount;
        const isPeak = index === peakIndex && !lit;
        return (
          <span
            key={index}
            data-zone={zone}
            className={`mic-meter__seg${lit ? " is-lit" : ""}${isPeak ? " is-peak" : ""}`}
          />
        );
      })}
    </div>
  );
}

const MIC_ACTIVE_RAMP_UP = 0.08;
const MIC_ACTIVE_RAMP_DOWN = 0.03;
// VU dynamics at the backend's ~40 ms tick: fast attack so peaks register,
// slower release so the bar glides back down instead of flickering.
const MIC_ATTACK_ALPHA = 0.55;
const MIC_RELEASE_ALPHA = 0.18;
// Peak-hold decay per tick — the marker falls from full to zero in ~2.5 s.
const MIC_PEAK_DECAY = 0.016;

function nextActive(prev: boolean, level: number): boolean {
  if (!prev && level >= MIC_ACTIVE_RAMP_UP) return true;
  if (prev && level <= MIC_ACTIVE_RAMP_DOWN) return false;
  return prev;
}

type SidecarErrorPayload = { kind?: string; permission?: string; hint?: string; message?: string };

function MicPicker({ microphone, microphones, onConfigChanged }: { microphone?: string | number | null; microphones: MicrophoneResult[]; onConfigChanged: Props["onConfigChanged"] }) {
  // The level check and echo are two different modes on one capture stream.
  // There used to be a single button turning both on at once: to look at the
  // meter you had to listen to yourself through the speakers and catch the
  // feedback. Hence two flags, while the stream lives as long as at least one of
  // them is on.
  const [checking, setChecking] = useState(false);
  const [echo, setEcho] = useState(false);
  const [busy, setBusy] = useState(false);
  const [devices, setDevices] = useState(microphones);
  const playback = useRef<AudioContext | null>(null);
  const nextPlayback = useRef(0);

  function closePlayback() {
    const context = playback.current;
    playback.current = null;
    nextPlayback.current = 0;
    if (context) void context.close();
  }

  // The device list is re-read on an event rather than on a timer: polling every
  // three seconds drove cpal enumeration on the audio thread the whole time the
  // settings were open — including with the window minimised to the tray. Both
  // moments when a microphone plugged in mid-session should appear are caught
  // precisely: focus returning to the window and the list itself being opened.
  const pendingRefresh = useRef(false);
  const disposed = useRef(false);
  const refreshDevices = useCallback(async () => {
    if (pendingRefresh.current) return;
    pendingRefresh.current = true;
    try {
      const list = await invoke<MicrophoneResult[]>("list_microphones");
      if (!disposed.current) setDevices(list);
    } catch { /* Keep the last successful enumeration during a reconnect. */ }
    finally { pendingRefresh.current = false; }
  }, []);

  useEffect(() => {
    disposed.current = false;
    const onFocus = () => void refreshDevices();
    void refreshDevices();
    window.addEventListener("focus", onFocus);
    return () => {
      disposed.current = true;
      window.removeEventListener("focus", onFocus);
    };
  }, [refreshDevices]);
  const [level, setLevel] = useState(0);
  const [peak, setPeak] = useState(0);
  const [micActive, setMicActive] = useState(false);
  const smoothRef = useRef(0);
  const peakRef = useRef(0);
  const prevActiveRef = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<{ text: string; kind: "running" | "stopped" } | null>(null);
  const statusTimerRef = useRef<number | null>(null);

  function resetMeter() {
    smoothRef.current = 0;
    peakRef.current = 0;
    prevActiveRef.current = false;
    setLevel(0);
    setPeak(0);
    setMicActive(false);
  }
  const options = [{ label: t("Системный микрофон по умолчанию"), value: null as string | number | null }, ...devices.map((mic) => ({ label: mic.name || mic.label || String(mic.id ?? mic.index), value: mic.id ?? mic.index ?? null }))];

  const selectedValue = typeof microphone === "number" || (typeof microphone === "string" && /^\d+$/.test(microphone))
    ? devices.find((mic) => mic.index === Number(microphone))?.id ?? microphone
    : microphone ?? null;
  if (selectedValue !== null && !options.some((option) => option.value === selectedValue)) {
    options.push({ label: t("Микрофон отключён"), value: selectedValue });
  }

  function setRunningStatus(text: string) {
    if (statusTimerRef.current !== null) {
      window.clearTimeout(statusTimerRef.current);
      statusTimerRef.current = null;
    }
    setStatus({ text, kind: "running" });
  }

  function setStoppedStatus(text: string, ttlMs = 1500) {
    if (statusTimerRef.current !== null) {
      window.clearTimeout(statusTimerRef.current);
      statusTimerRef.current = null;
    }
    setStatus({ text, kind: "stopped" });
    statusTimerRef.current = window.setTimeout(() => {
      statusTimerRef.current = null;
      setStatus((current) => (current?.kind === "stopped" ? null : current));
    }, ttlMs);
  }

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;
    const subscribe = <T,>(event: string, handler: (payload: T) => void) => {
      on<T>(event, handler).then((fn) => {
        if (cancelled) fn();
        else unlisteners.push(fn);
      });
    };
    subscribe<number[]>("microphone-test-audio", (samples) => {
      const context = playback.current;
      if (!context || context.state !== "running" || !samples.length) return;
      // Bound queued playback when the webview was stalled or hidden.
      if (nextPlayback.current > context.currentTime + 0.25) return;
      const buffer = context.createBuffer(1, samples.length, 16000);
      buffer.copyToChannel(new Float32Array(samples), 0);
      const source = context.createBufferSource();
      source.buffer = buffer;
      source.connect(context.destination);
      const at = Math.max(context.currentTime + 0.02, nextPlayback.current);
      source.start(at);
      nextPlayback.current = at + buffer.duration;
    });
    subscribe<{ level: number }>("microphone-test-level", (payload) => {
      const raw = Math.max(0, Math.min(1, payload.level ?? 0));
      // Fast-attack / slow-release smoothing so the meter fills left-to-right
      // with the current loudness (the incoming value is already perceptual and
      // backend-smoothed). This drives both the bar length and the active glow.
      const alpha = raw > smoothRef.current ? MIC_ATTACK_ALPHA : MIC_RELEASE_ALPHA;
      const next = raw * alpha + smoothRef.current * (1 - alpha);
      smoothRef.current = next;
      setLevel(next);
      // Peak-hold: jump up instantly, then decay so the loudest recent moment
      // stays marked ahead of the fill.
      peakRef.current = raw >= peakRef.current ? raw : Math.max(next, peakRef.current - MIC_PEAK_DECAY);
      setPeak(peakRef.current);
      const active = nextActive(prevActiveRef.current, next);
      if (active !== prevActiveRef.current) {
        prevActiveRef.current = active;
        setMicActive(active);
      }
    });
    // The modes are switched on by the button handlers — they know which of the
    // two was pressed. What is left here is only an external stop (hiding the
    // window, changing the device): there is one capture stream, and when it
    // ends both modes go out.
    subscribe<unknown>("microphone-test-started", () => { setError(null); });
    subscribe<unknown>("microphone-test-stopped", () => { closePlayback(); setChecking(false); setEcho(false); resetMeter(); });
    subscribe<SidecarErrorPayload>("app-error", (payload) => {
      // Permission events are shown by the banner in MainWindow — it has text
      // for the specific permission and a link into the right system settings
      // pane. There must be no such branch here: it labelled every one of them
      // as "no microphone access" with macOS instructions, even though the only
      // source of such events is Accessibility on macOS.
      if (payload?.permission) return;
      if (payload?.message) setError(String(payload.message));
    });
    subscribe<{ message?: string }>("microphone-test-failed", (payload) => {
      closePlayback();
      setChecking(false);
      setEcho(false);
      resetMeter();
      setError(payload?.message ?? t("Тест микрофона не удался"));
      setStoppedStatus(t("Ошибка теста микрофона"));
    });
    return () => {
      cancelled = true;
      closePlayback();
      void invoke("stop_microphone_test").catch(() => {});
      for (const fn of unlisteners) fn();
      if (statusTimerRef.current !== null) {
        window.clearTimeout(statusTimerRef.current);
        statusTimerRef.current = null;
      }
    };
  }, []);

  // The capture is shared by both modes: start is idempotent, and stop happens
  // only when the second mode is off as well — otherwise leaving echo would kill
  // the meter.
  async function startCapture(monitor: boolean) {
    await invoke("start_microphone_test", { microphone: microphone ?? null, monitor });
  }

  async function stopCapture() {
    closePlayback();
    setChecking(false);
    setEcho(false);
    resetMeter();
    await invoke("stop_microphone_test");
  }

  async function toggleCheck() {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      if (checking) {
        setChecking(false);
        if (!echo) await stopCapture();
        setStoppedStatus(t("Проверка микрофона остановлена"));
      } else {
        await startCapture(echo);
        setChecking(true);
        setRunningStatus(t("Проверка микрофона запущена"));
      }
    } catch (e) {
      await stopCapture().catch(() => {});
      setError(e instanceof Error ? e.message : String(e));
    } finally { setBusy(false); }
  }

  async function toggleEcho() {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      if (echo) {
        closePlayback();
        setEcho(false);
        await invoke("set_microphone_test_monitor", { enabled: false });
        if (!checking) await stopCapture();
        setStoppedStatus(t("Эхо выключено"));
      } else {
        // The context is opened before the capture starts: the browser's
        // autoplay gate lifts only inside a click handler.
        const context = new AudioContext();
        playback.current = context;
        await context.resume();
        await startCapture(true);
        setEcho(true);
        setRunningStatus(t("Эхо включено"));
      }
    } catch (e) {
      await stopCapture().catch(() => {});
      setError(e instanceof Error ? e.message : String(e));
    } finally { setBusy(false); }
  }

  async function selectMicrophone(value: string | number | null) {
    await stopCapture();
    await onConfigChanged({ microphone: value });
  }

  return (
    <div>
      <div className="mic-control">
        <CustomSelect className="custom-select--mic" value={selectedValue} options={options.map((option) => ({ ...option, icon: "mic" }))} onOpen={() => void refreshDevices()} onChange={(value) => void selectMicrophone(value).catch((e) => setError(String(e)))}/>
        {/* Headphones come first: the device list is to their left, and echo
            answers the first question about a new microphone ("can I be heard
            at all, and how?"), while the level meter refines the volume. */}
        <Hint text={t("Возвращает ваш голос обратно, чтобы вы слышали себя таким, каким вас слышит программа: шум, хрипы, гулкость комнаты. Только в наушниках: через колонки микрофон поймает сам себя.")}>
          <button className={`mic-test${echo ? " mic-test--active" : ""}`} type="button" disabled={busy} aria-pressed={echo} aria-label={t("Эхо")} onClick={() => void toggleEcho()}><Icon name="headphones" size={14}/></button>
        </Hint>
        <Hint text={t("Индикатор показывает уровень сигнала. Скажите что-нибудь: полоса должна доходить до середины и не упираться в край.")}>
          <button className={`mic-test${checking ? " mic-test--active" : ""}`} type="button" disabled={busy} aria-pressed={checking} aria-label={t("Проверка микрофона")} onClick={() => void toggleCheck()}><Icon name="mic" size={14}/></button>
        </Hint>
        {status && <span className={`mic-status-chip${status.kind === "stopped" ? " mic-status-chip--stop" : ""}`} role="status" aria-live="polite">{status.text}</span>}
        <MicMeter level={level} peak={peak} active={micActive}/>
      </div>
      {error && <div role="alert" style={{ marginTop: 6, color: "var(--err)", font: "500 11px/1.4 var(--font-sans)" }}>{error}</div>}
    </div>
  );
}

function TypingSpeedControl({ value, onConfigChanged }: { value?: number; onConfigChanged: Props["onConfigChanged"] }) {
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
        <span style={{ font: "500 12px/1 var(--font-sans)", color: "var(--text-2)", whiteSpace: "nowrap" }}>{t("симв/мин")}</span>
      </div>
    </div>
  );
}

export function SettingsPage({ config, microphones, models, onConfigChanged }: Props) {
  const model = config?.model || "large-v3";
  const autoPaste = config?.auto_paste ?? true;
  const autoStart = config?.auto_start ?? false;
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
  // Settings need only the selected model — the speech language and the device
  // choice follow from its properties (language, CPU-only).
  const selectedModelInfo = (models.length ? models : fallbackModels()).find((item) => item.id === model);
  const recordingMode = config?.recording_mode ?? "toggle";
  // One caption for every place the test's state is put into words: the
  // button's hint and the screen-reader line must say the same thing.
  const duckTestLabel = duckTest === "running" ? t("Проверяем приглушение…")
    : duckTest === "done" ? t("Громкость восстановлена")
    : duckTest === "error" ? duckTestError
    : t("Проверить");

  return (
    <div className="page">
      <PageHeader title={t("Настройки")}/>

      {/* 1. Capture row: hotkey · recording mode — one row split by a vrule.
          The model picker lives on its own page: here it duplicated the catalog,
          and with it downloading and deleting. */}
      <section className="card" style={{ padding: "12px 16px", marginBottom: 10 }}>
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
      </section>

      {/* 2. Languages — 2 cols. The names separate the two settings from each
          other, while the hint on the speech language answers the next
          question: what happens if you dictate in another one. */}
      <section className="card" style={{ padding: "12px 16px", marginBottom: 10 }}>
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
      </section>

      {/* 3. Microphone — own row (long device names) */}
      <section className="card" style={{ padding: "12px 16px", marginBottom: 10 }}>
        <div className="set-cell">
          <SetLabel title={t("Микрофон")}/>
          <MicPicker microphone={config?.microphone} microphones={microphones} onConfigChanged={onConfigChanged}/>
        </div>
      </section>

      {/* 4. Behaviour row: the paste settings as one sequence of checkboxes; a
          separate switch created a false visual hierarchy. */}
      <section className="card" style={{ padding: "12px 16px" }}>
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
            <button
              className="btn btn--ghost behavior-row__duck-button"
              type="button"
              data-state={duckTest}
              data-visible={duckOutput ? "true" : "false"}
              disabled={!duckOutput || duckTest === "running"}
              tabIndex={duckOutput ? undefined : -1}
              aria-hidden={duckOutput ? undefined : true}
              onClick={() => void testOutputDuck()}
              title={duckTestLabel}
              aria-label={duckTestLabel}
            >
              <Icon name={duckTest === "done" ? "check" : duckTest === "error" ? "x" : "test"} size={12}/>
            </button>
            <span className="sr-only" role="status">{duckTest === "idle" ? "" : duckTestLabel}</span>
          </div>
        </div>
      </section>

      {/* 5. Everything that is configured once or never. Collapsed on purpose:
          at the top level these controls cost a new user more than they saved an
          experienced one. */}
      <details className="card advanced" style={{ padding: "12px 16px", marginTop: 10 }}>
        <summary>
          <Icon name="chev-down" size={13}/>
          {t("Дополнительно")}
        </summary>

        <div className="advanced__main-row">
          <div className="set-cell">
            <SetLabel title={t("Устройство обработки")}/>
            <DevicePicker device={config?.device} cpuOnly={selectedModelInfo?.cpu_only} onConfigChanged={onConfigChanged}/>
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
          <span className="label-with-hint">
            <label className="checkbox-row">
              <input className="checkbox" type="checkbox" checked={autoStart} onChange={(e) => void onConfigChanged({ auto_start: e.target.checked })}/>
              {t("Запускать вместе с системой")}
            </label>
            <HintIcon text={t("Приложение запускается в фоне при входе в систему, горячая клавиша становится доступна сразу.")}/>
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
      </details>
    </div>
  );
}
