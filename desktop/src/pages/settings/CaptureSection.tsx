import { useEffect, useRef, useState } from "react";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { Segmented } from "../../components/Shell";
import { Icon } from "../../components/Icon";
import { Hint } from "../../components/Hint";
import { t } from "../../i18n";
import { DEFAULT_HOTKEY, normalizeHotkeyKey } from "../../hotkey";
import type { ConfigChanged } from "./controls";

function hotkeyLabel(hotkey: string | undefined, fallback: string) {
  return (hotkey || fallback).split("+").map((part) => part.trim()).filter(Boolean);
}

export function HotkeyDisplay({ hotkey, fallback = DEFAULT_HOTKEY, onConfigChanged }: {
  hotkey?: string;
  fallback?: string;
  onConfigChanged: ConfigChanged;
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

  // Reject macOS shortcuts needed to leave or hide the app before calling
  // the native `validate_hotkey` command.
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
            data-testid="hotkey-input"
            aria-label={t("Горячая клавиша")}
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
                      {i < arr.length - 1 && <span style={{ color: "var(--ink-mute)" }}>+</span>}
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
        {hotkeyLabel(hotkey, fallback).map((key, i, arr) => <span key={`${key}-${i}`} style={{ display: "inline-flex", alignItems: "center", gap: 6 }}><span className="kbd">{key}</span>{i < arr.length - 1 && <span style={{ color: "var(--ink-mute)" }}>+</span>}</span>)}
      </div>
      <Hint text={t("Изменить")}>
        <button
          className="btn btn--ghost hotkey-display__edit"
          type="button"
          aria-label={t("Изменить")}
          onClick={() => { setValue(hotkey || fallback); setEditing(true); }}
        ><Icon name="pencil" size={13}/></button>
      </Hint>
    </div>
  );
}

export function RecordingModeSegmented({ value, onConfigChanged }: { value: string; onConfigChanged: ConfigChanged }) {
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
