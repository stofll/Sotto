import { useEffect, useMemo, useRef, useState } from "react";
import { invoke, on } from "../bridge";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { PageHeader, Segmented } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Hint } from "../components/Hint";
import { CustomSelect, type SelectOption } from "../components/CustomSelect";
import { NumberField } from "../components/NumberField";
import type { ConfigResult, MicrophoneResult, ModelInfo } from "../bridge/types";
import { getLocale, isLocale, LOCALE_LABELS, LOCALES, setLocale, t, type Locale } from "../i18n";
import { DEFAULT_HOTKEY } from "../hotkey";
import { fallbackLanguage, fallbackModels, speechLanguages } from "./modelCatalog";
import { isTelemetryEnabled } from "./telemetrySettings";

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

  function normalizeKey(e: KeyboardEvent): string | null {
    // Modifier keys come through with both e.key and e.code reflecting the name.
    if (e.key === "Control" || e.code === "ControlLeft" || e.code === "ControlRight") return "ctrl";
    if (e.key === "Meta" || e.code === "MetaLeft" || e.code === "MetaRight") return "cmd";
    if (e.key === "Alt" || e.code === "AltLeft" || e.code === "AltRight") return "alt";
    if (e.key === "Shift" || e.code === "ShiftLeft" || e.code === "ShiftRight") return "shift";
    const named: Record<string, string> = {
      Space: "space", Escape: "escape", Enter: "enter", Tab: "tab",
      Backspace: "backspace", Delete: "delete",
      ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down",
      Home: "home", End: "end", PageUp: "pageup", PageDown: "pagedown",
    };
    if (named[e.code]) return named[e.code];
    if (named[e.key]) return named[e.key];
    if (/^F\d{1,2}$/.test(e.key)) return e.key.toLowerCase();
    if (/^F\d{1,2}$/.test(e.code)) return e.code.toLowerCase();
    // Plain printable characters (latin letters/digits/symbols).
    if (e.key.length === 1) return e.key.toLowerCase();
    return null;
  }

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
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        cancelRecording();
        return;
      }
      const token = normalizeKey(e);
      if (!token) return;
      // Suppress browser/shortcut interference (Cmd+Space → Spotlight, etc.)
      e.preventDefault();
      e.stopPropagation();
      // Ignore OS key auto-repeat: the combo is captured once, on the initial
      // press of the non-modifier key, using whatever modifiers are held.
      if (e.repeat) return;
      pressedRef.current.add(token);
      setPressedKeys(new Set(pressedRef.current));
      // A non-modifier is the "main" key — snapshot the current chord and
      // commit immediately. This avoids races between an idle timer and
      // keyup-driven mutations that caused combos to capture unreliably.
      if (hasNonModifier(pressedRef.current)) {
        const combo = formatCombo(pressedRef.current);
        pressedRef.current = new Set();
        void commit(combo);
      }
    }
    function onKeyUp(e: KeyboardEvent) {
      const token = normalizeKey(e);
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
              if (e.key === "Escape") { cancelRecording(); setEditing(false); }
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

// Язык интерфейса. Отдельно от LanguagePicker намеренно: тот про язык речи,
// и путать их дорого — выбрав «English» в надежде переключить интерфейс,
// пользователь сломает распознавание русской диктовки.
function UiLanguagePicker({ value, onConfigChanged }: { value?: Locale; onConfigChanged: (patch: Partial<ConfigResult>) => Promise<unknown> }) {
  const current = value ?? getLocale();
  // Обёртка — ради высоты: Segmented стилизован инлайном, дотянуться до его
  // кнопок можно только через класс снаружи.
  return (
    <div className="lang-row__ui-language">
      <Segmented
        value={current}
        options={LOCALES.map((locale) => ({ value: locale, label: LOCALE_LABELS[locale] }))}
        onChange={(next) => {
          if (!isLocale(next)) return;
          // Применяем сразу, не дожидаясь ответа: сохранение может занять
          // сотни миллисекунд, а переключатель должен отзываться мгновенно.
          setLocale(next);
          void onConfigChanged({ ui_language: next });
        }}
      />
    </div>
  );
}

function LanguagePicker({ language, model, models, onConfigChanged }: { language?: string; model?: ModelInfo; models: ModelInfo[]; onConfigChanged: Props["onConfigChanged"] }) {
  // Модель с закрытым списком языков сама решает за этот список: и
  // английские сборки Whisper, и GigaAM на чужом языке выдают не ошибку, а
  // мусор.
  const correction = fallbackLanguage(model, language);
  useEffect(() => {
    if (correction) void onConfigChanged({ language: correction });
  }, [correction, onConfigChanged]);
  // Тот же список, что и в каталоге: странно уметь скачать немецкую модель и
  // не уметь сказать, что диктуешь по-немецки. Флагов здесь нет намеренно —
  // язык не страна, и у английского с арабским их по десятку.
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

// Смена устройства перезагружает модель на стороне Rust — это единственный
// момент, когда whisper.cpp применяет use_gpu. Пока идёт перезагрузка,
// прилетают штатные model-loading / model-ready.
function DevicePicker({ device, cpuOnly, onConfigChanged }: { device?: string; cpuOnly?: boolean; onConfigChanged: Props["onConfigChanged"] }) {
  // Всё, кроме явного "cpu", — GPU: так же считает Rust (`resolve_device`),
  // включая унаследованное значение "cuda".
  const current = cpuOnly ? "cpu" : (device === "cpu" ? "cpu" : "gpu");
  // Выбор из двух взаимоисключающих значений — это тумблер, а не список:
  // оба варианта видны сразу, и раскрывать меню ради них незачем. У CPU-only
  // модели тумблер гасится, и рядом появляется короткое объяснение «почему».
  return (
    <div className="device-picker">
      <Segmented
        value={current}
        disabled={cpuOnly}
        options={[
          { value: "gpu", label: "GPU", icon: "gpu" },
          { value: "cpu", label: "CPU", icon: "cpu" },
        ]}
        onChange={(next) => { if (!cpuOnly) void onConfigChanged({ device: next as "gpu" | "cpu" }); }}
      />
      {cpuOnly && <span className="device-picker__note">{t("модель работает только на CPU")}</span>}
    </div>
  );
}

const SOUND_VOLUME_PRESETS = () => ([
  { label: t("Тихо"), value: 0.15 },
  { label: t("Средне"), value: 0.35 },
  { label: t("Громко"), value: 0.7 },
]);
const DEFAULT_SOUND_VOLUME = 0.35;
// Псевдо-громкость для пункта «Выключено». Для пользователя «включены ли сигналы»
// и «насколько громко» — один выбор, поэтому и контрол один: отдельный тумблер
// стоил второго клика и держал селект в disabled, ничего при этом не решая.
const SOUND_OFF = 0;

// Звуковые сигналы диктовки: старт записи, конец, вставка текста, ошибка.
// Значения по умолчанию продублированы в src-tauri/src/sounds.rs.
function SoundFeedbackControl({ enabled, volume, onConfigChanged }: { enabled: boolean; volume: number; onConfigChanged: Props["onConfigChanged"] }) {
  const presets = SOUND_VOLUME_PRESETS();
  const knownVolume = presets.some((preset) => preset.value === volume) ? volume : DEFAULT_SOUND_VOLUME;
  // Проигрываем выбранное значение сразу: громкость на слух не выбирается
  // по названию пресета.
  function preview(nextVolume: number) {
    void tauriInvoke("preview_sound_cue", { cue: "done", volume: nextVolume }).catch(() => {});
  }

  // «Выключено» не трогает sound_volume: вернув сигналы, пользователь получает ту
  // же громкость, которую выбирал раньше.
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

// Продублировано в src-tauri/src/history.rs (RetentionPolicy::default).
const DEFAULT_HISTORY_RETENTION_DAYS = 30;
const DEFAULT_HISTORY_MAX_ENTRIES = 1000;
const MAX_HISTORY_RETENTION_DAYS = 3650;

function HistoryRetentionControl({ days, maxEntries, onConfigChanged }: { days: number; maxEntries: number; onConfigChanged: Props["onConfigChanged"] }) {
  const [draftDays, setDraftDays] = useState(String(days));
  const [draftEntries, setDraftEntries] = useState(String(maxEntries));

  useEffect(() => { setDraftDays(String(days)); }, [days]);
  useEffect(() => { setDraftEntries(String(maxEntries)); }, [maxEntries]);

  // Совпадает с проверкой в Rust: значение вне диапазона там откатывается к
  // дефолту, так что до конфига оно доезжать не должно.
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
  const [testing, setTesting] = useState(false);
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
  const options = [{ label: t("Системный микрофон по умолчанию"), value: null as string | number | null }, ...microphones.map((mic) => ({ label: mic.name || mic.label || String(mic.id ?? mic.index), value: mic.id ?? mic.index ?? null }))];

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
    subscribe<unknown>("microphone-test-started", () => { setTesting(true); setError(null); setRunningStatus(t("Тест микрофона запущен")); });
    subscribe<unknown>("microphone-test-stopped", () => { setTesting(false); resetMeter(); setStoppedStatus(t("Тест микрофона остановлен")); });
    subscribe<SidecarErrorPayload>("app-error", (payload) => {
      if (payload?.kind === "permission") {
        const detail = payload.message ? ` (${payload.message})` : "";
        setError(t("Нет доступа к микрофону. Откройте «Системные настройки → Конфиденциальность → Микрофон» и разрешите доступ для приложения.{p0}", { p0: detail }));
      } else if (payload?.message) {
        setError(String(payload.message));
      }
    });
    subscribe<{ message?: string }>("microphone-test-failed", (payload) => {
      setTesting(false);
      resetMeter();
      setError(payload?.message ?? t("Тест микрофона не удался"));
      setStoppedStatus(t("Ошибка теста микрофона"));
    });
    return () => {
      cancelled = true;
      for (const fn of unlisteners) fn();
      if (statusTimerRef.current !== null) {
        window.clearTimeout(statusTimerRef.current);
        statusTimerRef.current = null;
      }
    };
  }, []);

  async function toggleTest() {
    setError(null);
    if (testing) {
      try {
        await invoke("stop_microphone_test");
        setTesting(false);
        resetMeter();
        setStoppedStatus(t("Тест микрофона остановлен"));
      } catch (e) {
        // Force UI consistent even if backend is wedged.
        setTesting(false);
        resetMeter();
        setError(e instanceof Error ? e.message : String(e));
        setStoppedStatus(t("Ошибка остановки теста"));
      }
      return;
    }
    try {
      await invoke("start_microphone_test", { microphone });
      setTesting(true);
      setRunningStatus(t("Тест микрофона запущен"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <div>
      <div className="mic-control">
        <CustomSelect className="custom-select--mic" value={microphone ?? null} options={options.map((option) => ({ ...option, icon: "mic" }))} onChange={(value) => void onConfigChanged({ microphone: value })}/>
        <button className={`mic-test${testing ? " mic-test--active" : ""}`} type="button" onClick={() => void toggleTest()} title={t("Проверить микрофон")}><Icon name="mic" size={14}/></button>
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
  // Настройкам нужна только выбранная модель — за её характеристиками
  // (язык, CPU-only) следуют язык речи и выбор устройства.
  const selectedModelInfo = (models.length ? models : fallbackModels()).find((item) => item.id === model);
  const recordingMode = config?.recording_mode ?? "toggle";

  return (
    <div className="page">
      <PageHeader title={t("Настройки")}/>

      {/* 1. Capture row: hotkey · recording mode — одна строка через vrule.
          Выбор модели живёт на своей странице: здесь он дублировал каталог,
          а вместе с ним и скачивание с удалением. */}
      <section className="card" style={{ padding: "12px 16px", marginBottom: 10 }}>
        <div className="capture-row">
          <div className="set-cell">
            <SetLabel title={t("Горячая клавиша")} hint={t("Диктовка. Пойдёт ли текст в LLM, решает режим обработки на вкладке «ИИ».")}/>
            <HotkeyDisplay hotkey={config?.hotkey} fallback={DEFAULT_HOTKEY} onConfigChanged={onConfigChanged}/>
          </div>
          <div className="vrule"/>
          <div className="set-cell">
            <SetLabel title={t("Режим записи")}/>
            <RecordingModeSegmented value={recordingMode} onConfigChanged={onConfigChanged}/>
          </div>
        </div>
      </section>

      {/* 2. Languages — 2 cols. Названия разводят две настройки между собой,
          а подсказка у языка речи отвечает на следующий вопрос: что будет,
          если продиктовать не на нём. */}
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

      {/* 4. Behaviour row: настройки вставки — одна последовательность
          чекбоксов; отдельный тумблер создавал ложную визуальную иерархию. */}
      <section className="card" style={{ padding: "12px 16px" }}>
        <div className="behavior-row behavior-row--primary">
          <div className="set-cell behavior-row__paste-options">
            <span className="label-with-hint">
              <label className="checkbox-row">
                <input className="checkbox" type="checkbox" checked={autoPaste} onChange={(e) => void onConfigChanged({ auto_paste: e.target.checked })}/>
                {t("Авто-вставка текста")}
              </label>
              <HintIcon text={t("Сразу вставлять распознанный текст в активное поле. Если выключить, текст останется только в буфере обмена.")}/>
            </span>
            <label className="checkbox-row" style={{ color: autoPaste ? "var(--ink-mute)" : "var(--ink-faint)" }}>
              <input className="checkbox" type="checkbox" disabled={!autoPaste} checked={config?.paste_trailing_space ?? false} onChange={(e) => void onConfigChanged({ paste_trailing_space: e.target.checked })}/>
              {t("Пробел в конце")}
            </label>
            <label className="checkbox-row" style={{ color: autoPaste ? "var(--ink-mute)" : "var(--ink-faint)" }} title={t("Нажать Enter сразу после вставки — отправит сообщение в чате или запустит поиск.")}>
              <input className="checkbox" type="checkbox" disabled={!autoPaste} checked={config?.paste_auto_submit ?? false} onChange={(e) => void onConfigChanged({ paste_auto_submit: e.target.checked })}/>
              {t("Enter после вставки")}
            </label>
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
          {/* Приглушение — не обработка записи, а то, что приложение делает с
              системой, пока пишет; и переключают его ситуативно: в наушниках
              не нужно, с колонок нужно. Отсюда соседство со вставкой, а не
              место в «Дополнительно». */}
          <div className="set-cell behavior-row__duck">
            <span className="label-with-hint">
              <label className="checkbox-row">
                <input className="checkbox" type="checkbox" checked={duckOutput} onChange={(e) => void onConfigChanged({ duck_output_while_recording: e.target.checked })}/>
                {t("Приглушать звук на время записи")}
              </label>
              <HintIcon text={t("На время записи убавить общую громкость и вернуть её после. Нужно, если пишете с колонок: звук из них попадает в микрофон.")}/>
            </span>
            {/* Проверка нужна ровно один раз — когда включили. Пока выключено,
                кнопке в этой строке делать нечего. */}
            {duckOutput && (
              <>
                <button className="btn btn--ghost" type="button" disabled={duckTest === "running"} onClick={() => void testOutputDuck()} style={{ height: 26, padding: "0 9px", fontSize: 11 }}>
                  {duckTest === "running" ? t("Проверяем приглушение…") : t("Проверить")}
                </button>
                {duckTest === "done" && <span role="status" style={{ font: "500 11px/1.3 var(--font-sans)", color: "var(--ok)" }}>{t("Громкость восстановлена")}</span>}
                {duckTest === "error" && <span role="alert" title={duckTestError} style={{ font: "500 11px/1.3 var(--font-sans)", color: "var(--err)", maxWidth: 180, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{duckTestError}</span>}
              </>
            )}
          </div>
        </div>
      </section>

      {/* 5. Всё, что настраивают один раз или не настраивают никогда. Свёрнуто
          намеренно: на верхнем уровне эти контролы стоили новому
          пользователю больше, чем экономили опытному. */}
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
        </div>

        {/* Автозапуск ставят один раз за установку — ровно тот случай, ради
            которого блок и свёрнут. Обрезки тишины здесь больше нет: vad.rs
            сам отказывается резать, когда речь не найдена или экономия меньше
            секунды, так что выключателю было нечего чинить. Ключ trim_silence
            в config.json по-прежнему читается — как отладочный.
            Согласие на телеметрию стоит здесь же: это такой же выключатель
            «поставил один раз», и отдельный подраздел под одну строку был
            тяжелее самой строки. */}
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
