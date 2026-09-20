import { useEffect, useLayoutEffect, useState } from "react";
import type { CSSProperties } from "react";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { Icon } from "../components/Icon";
import { subscribe, onRecordingStateChange, type RecordingState } from "../bridge";
import { invoke } from "../bridge/invoke";
import { isWindowsOs } from "../bridge/platform";
import type { ConfigResult, MicrophoneResult, RuntimeStatusResult } from "../bridge/types";
import { applyLocaleFromConfig, t, useLocale } from "../i18n";
import { applyAccent, resolveAccent } from "../accent";
import { actualDeviceLabel, actualEngineLabel, actualModelLabel } from "../pages/runtimePresentation";

type TabId = "settings" | "text" | "ai" | "stats" | "info";

const LANGUAGE_LABEL = (): Record<string, string> => ({
  ru: t("Русский"),
  en: "English",
  auto: t("Авто"),
});

// Local accelerated backends share one label; an unloaded route has no device.
function deviceLabel(device?: string | null) {
  if (!device || device === "—") return "—";
  if (device === "cloud") return t("Облако");
  return device === "cpu" ? "CPU" : "GPU";
}

function statusText(state: RecordingState, runtime: RuntimeStatusResult | null) {
  if (state === "recording") return t("Идёт запись");
  if (state === "processing") return t("Распознаю");
  if (state === "loading" || runtime?.state === "loading") return t("Загружаю модель");
  if (state === "error") return t("Ошибка");
  const cloudRouteReady = runtime?.active_engine === "cloud-stt" && !!runtime.active_model;
  if (runtime?.model_loaded === false && !cloudRouteReady) return t("Модель не загружена");
  return t("Готово");
}

function rowButtonStyle(extra?: CSSProperties): CSSProperties {
  return {
    width: "100%",
    appearance: "none",
    border: 0,
    background: "transparent",
    color: "var(--ink)",
    display: "flex",
    alignItems: "center",
    gap: 10,
    padding: "8px 8px",
    borderRadius: 6,
    cursor: "pointer",
    textAlign: "left",
    ...extra,
  };
}

export function TrayApp() {
  useLocale();
  const [recordingState, setRecordingState] = useState<RecordingState>("idle");
  const [config, setConfig] = useState<ConfigResult | null>(null);
  const [microphones, setMicrophones] = useState<MicrophoneResult[]>([]);
  const [runtime, setRuntime] = useState<RuntimeStatusResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Whether the popup this window drives can exist. The commands that drive it
  // are registered under `#[cfg(windows)]`, so the popup-only controls read the
  // OS from `get_runtime_status` (fetched on mount below).
  //
  // An unreported OS counts as Windows rather than as "not Windows". `runtime`
  // is null for the first tick after mount, and tray.html is only ever loaded
  // by `show_tray_popup`, which is Windows-only — so Windows is the platform
  // that actually renders this, and reading that tick as non-Windows made the
  // «Скрыть меню» button appear a frame late and, worse, let a menu click in
  // that frame skip `hide_tray_popup`, leaving an always-on-top popup over the
  // main window. Elsewhere the same tick can send one stray command, which is
  // the rejection `openMain` already swallows.
  const os = runtime?.os;
  const isWindows = isWindowsOs(os) || os === undefined;
  const isRecording = recordingState === "recording";

  useEffect(() => {
    document.documentElement.dataset.theme = config?.theme ?? "dark";
    applyAccent(resolveAccent(config?.ui_accent));
  }, [config?.ui_accent, config?.theme]);

  useLayoutEffect(() => {
    const htmlBg = document.documentElement.style.background;
    const htmlOverflow = document.documentElement.style.overflow;
    const bodyBg = document.body.style.background;
    const bodyOverflow = document.body.style.overflow;
    const root = document.getElementById("root");
    const rootBg = root?.style.background;
    const rootOverflow = root?.style.overflow;
    document.documentElement.classList.add("tray-window");
    document.documentElement.style.background = "transparent";
    document.documentElement.style.overflow = "hidden";
    document.body.style.background = "transparent";
    document.body.style.overflow = "hidden";
    if (root) root.style.background = "transparent";
    if (root) root.style.overflow = "hidden";
    return () => {
      document.documentElement.style.background = htmlBg;
      document.documentElement.style.overflow = htmlOverflow;
      document.documentElement.classList.remove("tray-window");
      document.body.style.background = bodyBg;
      document.body.style.overflow = bodyOverflow;
      if (root) root.style.background = rootBg ?? "";
      if (root) root.style.overflow = rootOverflow ?? "";
    };
  }, []);

  useEffect(() => onRecordingStateChange(setRecordingState), []);

  useEffect(() => {
    let mounted = true;
    let runtimeRequest = 0;
    const refreshRuntime = () => {
      const request = ++runtimeRequest;
      void invoke<RuntimeStatusResult>("get_runtime_status").then((value) => {
        // A slow earlier snapshot must not overwrite a later model event.
        if (!mounted || request !== runtimeRequest) return;
        setRuntime(value);
        setRecordingState((current) => current === "loading" ? "idle" : current);
      }).catch(() => {});
    };
    void Promise.allSettled([
      invoke<ConfigResult>("get_config").then((value) => { if (mounted) { setConfig(value); applyLocaleFromConfig(value.ui_language); } }),
      invoke<MicrophoneResult[]>("list_microphones").then((value) => { if (mounted) setMicrophones(value); }),
    ]);
    refreshRuntime();
    const unlisteners: Array<() => void> = [];
    unlisteners.push(subscribe<ConfigResult>("config-updated", (next) => {
      setConfig(next);
      applyLocaleFromConfig(next.ui_language);
      refreshRuntime();
    }));
    unlisteners.push(subscribe<unknown>("whisper-loading", () => {
      ++runtimeRequest;
    }));
    for (const event of ["whisper-ready", "model-ready", "model-unloaded", "model-restored"]) {
      unlisteners.push(subscribe<unknown>(event, refreshRuntime));
    }
    unlisteners.push(subscribe<{ message?: string }>("whisper-load-failed", (payload) => {
      setError(payload.message ?? t("Не удалось загрузить модель"));
    }));
    return () => {
      mounted = false;
      unlisteners.forEach((stop) => stop());
    };
  }, []);

  // The popup window only exists on Windows (windows/tray_popup.rs): there
  // `hide_tray_popup` dismisses the popup before the main window takes focus.
  // Elsewhere `focus_main_window` alone shows the main window.
  async function openMain(tab: TabId) {
    if (isWindows) await tauriInvoke("hide_tray_popup").catch(() => {});
    await tauriInvoke("focus_main_window", { tab }).catch((e) => setError(e instanceof Error ? e.message : String(e)));
  }

  async function saveConfig(patch: Partial<ConfigResult>) {
    setError(null);
    try {
      const result = await invoke<ConfigResult>("save_config", { patch });
      setConfig(result);
      await emit("config-updated", result).catch(() => {});
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  const currentMic = microphones.find((mic) => String(mic.id ?? mic.index) === String(config?.microphone));
  const micLabel = currentMic?.name ?? currentMic?.label ?? t("Системный");
  const modelLabel = actualModelLabel(runtime, t("Модель не загружена"));
  const statusLabel = statusText(recordingState, runtime);
  const subtitle = [statusLabel, statusLabel === modelLabel ? null : modelLabel, actualEngineLabel(runtime), deviceLabel(actualDeviceLabel(runtime))].filter(Boolean).join(" · ");

  return (
    <div className="app-frame" style={{ width: "100%", height: "100%", background: "transparent", position: "relative", paddingBottom: 7, overflow: "hidden" }}>
      <div style={{ position: "relative", background: "var(--bg-3)", borderRadius: 12, border: "1px solid var(--line-strong)", boxShadow: "0 24px 64px rgba(0,0,0,0.5), 0 1px 0 rgba(255,255,255,0.04) inset", overflow: "hidden", fontFamily: "var(--font-sans)" }}>
        <div style={{ padding: "14px 16px 12px", background: "linear-gradient(160deg, rgba(246,169,59,0.10), rgba(246,169,59,0.02))", borderBottom: "1px solid var(--line)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ font: "600 13px/1 var(--font-sans)" }}>Sotto</div>
              <div style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 4 }}><span style={{ width: 6, height: 6, borderRadius: "50%", background: isRecording ? "var(--rec)" : recordingState === "loading" ? "var(--accent)" : "var(--ok)" }}/><span style={{ font: "500 11px/1 var(--font-mono)", color: "var(--ink-dim)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{subtitle}</span></div>
            </div>
          </div>
        </div>
        {error && <div style={{ padding: "14px 14px 8px" }}>
          <div role="alert" style={{ marginTop: 8, color: "var(--err)", font: "500 11px/1.35 var(--font-sans)" }}>{error}</div>
        </div>}
        <div style={{ padding: "0 14px 8px", display: "flex", flexDirection: "column", gap: 2 }}>
          {[
            { icon: "mic", label: t("Микрофон"), right: micLabel, tab: "settings" as TabId },
            { icon: "cpu", label: t("Модель"), right: modelLabel, tab: "settings" as TabId },
            { icon: "globe", label: t("Язык"), right: LANGUAGE_LABEL()[config?.language ?? "ru"] ?? config?.language ?? t("Русский"), tab: "settings" as TabId },
          ].map((row) => <button key={row.label} style={rowButtonStyle()} onClick={() => void openMain(row.tab)}><span style={{ color: "var(--ink-dim)", display: "flex" }}><Icon name={row.icon} size={14}/></span><span style={{ font: "500 12px/1 var(--font-sans)", color: "var(--ink)" }}>{row.label}</span><span style={{ marginLeft: "auto", font: "500 11px/1 var(--font-mono)", color: "var(--ink-mute)", maxWidth: 120, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{row.right}</span><span style={{ color: "var(--ink-mute)" }}><Icon name="chev" size={13}/></span></button>)}
        </div>
        <div role="menu" style={{ borderTop: "1px solid var(--line)", padding: 6, display: "flex", flexDirection: "column", gap: 1 }}>
          {[
            { icon: "sliders", label: t("Настройки"), right: "Ctrl+Win+,", action: () => openMain("settings") },
            { icon: "chart", label: t("Статистика"), action: () => openMain("stats") },
            { icon: "replace", label: config?.replacements_paused ? t("Возобновить замены") : t("Пауза замен"), action: () => saveConfig({ replacements_paused: !(config?.replacements_paused ?? false) }) },
            { icon: "info", label: t("Справка"), action: () => openMain("info") },
          ].map((item) => <button role="menuitem" key={item.label} style={rowButtonStyle({ padding: "8px 10px" })} onClick={() => void item.action()}><span style={{ color: "var(--ink-dim)", display: "flex" }}><Icon name={item.icon} size={14}/></span><span style={{ font: "500 12px/1 var(--font-sans)", color: "var(--ink)", flex: 1 }}>{item.label}</span>{item.right && <span className="mono" style={{ font: "500 10px/1 var(--font-mono)", color: "var(--ink-mute)" }}>{item.right}</span>}</button>)}
        </div>
        {/* The popup exists only on Windows (tray_popup.rs): a control that
            does nothing must not render on the other platforms. */}
        <div style={{ borderTop: "1px solid var(--line)", padding: "8px 16px", display: "flex", alignItems: "center" }}>
          {isWindows && <button style={{ appearance: "none", border: 0, background: "transparent", cursor: "pointer", font: "500 12px/1 var(--font-sans)", color: "var(--ink-dim)", padding: 0 }} onClick={() => tauriInvoke("hide_tray_popup").catch(() => {})}>{t("Скрыть меню")}</button>}
        </div>
      </div>
      <div style={{ position: "absolute", bottom: 1, right: 34, width: 12, height: 12, background: "var(--bg-3)", transform: "rotate(45deg)", borderRight: "1px solid var(--line-strong)", borderBottom: "1px solid var(--line-strong)" }}/>
    </div>
  );
}
