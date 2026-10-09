import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { invoke, subscribe, onRecordingStateChange, type RecordingState } from "./bridge";
import { getStats } from "./bridge/stats";
import type { ApiKeyStatus, AppVersionResult, ConfigChange, ConfigResult, MicrophoneResult, ModelInfo, RuntimeStatusResult, StatsResult } from "./bridge/types";
import { Card, Sidebar, TitleBar, type TabId, type DownloadProgress } from "./components/Shell";
import { Icon } from "./components/Icon";
import { WhatsNewDialog } from "./components/WhatsNewDialog";
import { AccessibilityNotice } from "./components/AccessibilityNotice";
import { UpdateNotice } from "./components/UpdateNotice";
import { TelemetryConsentCard } from "./pages/settings/TelemetryControl";
import { shouldOfferTelemetryConsent } from "./pages/telemetrySettings";
import { applyAccent, resolveAccent, storedAccent } from "./accent";

const COLLAPSE_STORAGE_KEY = "sotto.ui.sidebarCollapsed";
const AUTO_COLLAPSE_BELOW = 1100;
import { SettingsPage } from "./pages/SettingsPage";
import { ModelsPage } from "./pages/ModelsPage";
import { AiPage } from "./pages/AiPage";
import { IntegrationsPage } from "./pages/IntegrationsPage";
import { InfoPage } from "./pages/InfoPage";
import { StatsPage } from "./pages/StatsPage";
import { TextPage } from "./pages/TextPage";
import { actualModelLabel } from "./pages/runtimePresentation";
import { staleBuiltInPrompt } from "./pages/aiShared";
import { applyLocaleFromConfig, t, useLocale } from "./i18n";
import type { OnboardingProps } from "./onboarding/Onboarding";
import { onboardingExitTab } from "./onboarding/modelChoices";

const Onboarding = lazy(() => import("./onboarding/Onboarding").catch(() => ({ default: OnboardingLoadError })));

function OnboardingLoadError({ active, config, models, onConfigChanged, onNavigate, onEnd }: OnboardingProps) {
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  useEffect(() => { if (!active) onEnd(); }, [active, onEnd]);
  if (!active) return null;
  async function skip() {
    setBusy(true);
    setFailed(null);
    let reason = "";
    const saved = await onConfigChanged({ onboarding_completed: true }, (message) => { reason = message; });
    setBusy(false);
    if (saved) onNavigate(onboardingExitTab(config, models, false));
    else setFailed(reason);
  }
  return <div className="loading-state" role="alert">
    <div className="onboarding-fallback">
      <p>{t("Не удалось открыть введение.")}</p>
      <div className="onboarding-fallback__actions">
        <button className="btn btn--primary" type="button" disabled={busy} onClick={() => window.location.reload()}>{t("Перезагрузить")}</button>
        <button className="btn btn--ghost" type="button" disabled={busy} onClick={() => void skip()}>{t("Пропустить введение")}</button>
      </div>
      {failed !== null && <p className="inline-error">{t("Не удалось сохранить настройку: {p0}", { p0: failed })}</p>}
    </div>
  </div>;
}



// macOS URL schemes that deep-link into Privacy & Security panes. Opening one
// via System Settings' `x-apple.systempreferences:` handler opens the section
// where they can grant Microphone / Accessibility to Sotto.
const PRIVACY_URLS: Record<string, string> = {
  microphone: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
  accessibility: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
};

function openPrivacyPane(permission: string) {
  const url = PRIVACY_URLS[permission] ?? "x-apple.systempreferences:com.apple.preference.security?Privacy";
  // The Rust side `open_url` shells out to `open`/`xdg-open`/`start`.
  invoke<null>("open_url", { url }).catch(() => {/* ignore */});
}

/** An entry the history page should bring into view. `seq` makes a second
 *  request for the same entry count as new. */
type HistoryFocus = { id: number; seq: number };

function pageFor(tab: TabId, data: {
  config: ConfigResult | null;
  historyFocus: HistoryFocus | null;
  version: string | null;
  textPreviewDraft: string | null;
  onTextPreviewDraftChange: (text: string) => void;
  stats: StatsResult | null;
  microphones: MicrophoneResult[];
  models: ModelInfo[];
  runtime: RuntimeStatusResult | null;
  apiKeys: ApiKeyStatus;
  onConfigChanged: (change: ConfigChange, onError?: (message: string) => void) => Promise<ConfigResult | null>;
  onNavigate: (tab: TabId) => void;
  onApiKeysChanged: (next: ApiKeyStatus) => void;
  onModelsChanged: (models: ModelInfo[]) => void;
  onStatsRefresh: () => Promise<void>;
  onStartOnboarding: () => void;
  updateInstallRequested: boolean;
  onUpdateInstallRequestHandled: () => void;
}) {
  switch (tab) {
    case "settings": return <SettingsPage config={data.config} microphones={data.microphones} models={data.models} portable={data.runtime?.portable} onConfigChanged={data.onConfigChanged}/>;
    case "models": return <ModelsPage models={data.models} config={data.config} onConfigChanged={data.onConfigChanged} onModelsChanged={data.onModelsChanged}/>;
    case "text": return <TextPage config={data.config} onConfigChanged={data.onConfigChanged} previewDraft={data.textPreviewDraft} onPreviewDraftChange={data.onTextPreviewDraftChange}/>;
    case "ai": return <AiPage config={data.config?.ai_processing ?? null} apiKeys={data.apiKeys} onConfigChanged={data.onConfigChanged} onNavigate={(t) => data.onNavigate(t)}/>;
    case "integrations": return <IntegrationsPage config={data.config?.ai_processing ?? null} apiKeys={data.apiKeys} onConfigChanged={data.onConfigChanged} onApiKeysChanged={data.onApiKeysChanged}/>;
    case "history": return <HistoryPageLoader focus={data.historyFocus}/>;
    case "stats": return <StatsPage stats={data.stats} typingSpeedCpm={data.config?.typing_speed_cpm} onRefresh={data.onStatsRefresh}/>;
    case "info": return <InfoPage version={data.version} config={data.config} onConfigChanged={data.onConfigChanged} onStartOnboarding={data.onStartOnboarding} installRequested={data.updateInstallRequested} onInstallRequestHandled={data.onUpdateInstallRequestHandled}/>;
  }
}

function HistoryPageLoader({ focus }: { focus: HistoryFocus | null }) {
  const [Page, setPage] = useState<typeof import("./pages/HistoryPage")["HistoryPage"] | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let disposed = false;
    void import("./pages/HistoryPage")
      .then(({ HistoryPage }) => { if (!disposed) setPage(() => HistoryPage); })
      .catch(() => { if (!disposed) setFailed(true); });
    return () => { disposed = true; };
  }, []);
  if (Page) return <Page focus={focus}/>;
  return <div className="loading-state" role={failed ? "alert" : "status"}>
    {failed ? <div>
      <p>{t("Не удалось открыть историю.")}</p>
      <p>{t("Если перезагрузка не помогла, полностью закройте Sotto и запустите снова.")}</p>
      <button type="button" className="btn btn--ghost" onClick={() => window.location.reload()}>
        {t("Перезагрузить")}
      </button>
    </div> : t("Загрузка…")}
  </div>;
}

export function MainWindow() {
  // One language subscription at the root: t() reads module state, so
  // re-rendering the root is enough for the whole tree.
  useLocale();
  const [onboardingToolbar, setOnboardingToolbar] = useState<HTMLDivElement | null>(null);
  const [textPreviewDraft, setTextPreviewDraft] = useState<string | null>(null);
  const [tab, setTab] = useState<TabId>("settings");
  // «Обновить сейчас» on the update notice: the Help page's updates card
  // installs once its check confirms the update, then clears this. Leaving
  // the page first spends it too: a later visit must not install unasked.
  const [updateInstallRequested, setUpdateInstallRequested] = useState(false);
  useEffect(() => { if (tab !== "info") setUpdateInstallRequested(false); }, [tab]);
  const [historyFocus, setHistoryFocus] = useState<HistoryFocus | null>(null);
  // A request to show an entry is spent once the user leaves the page:
  // coming back later must not jump to it again.
  useEffect(() => { if (tab !== "history") setHistoryFocus(null); }, [tab]);
  const [theme, setTheme] = useState<"dark" | "light">("dark");
  const [version, setVersion] = useState<string | null>(null);
  const [config, setConfig] = useState<ConfigResult | null>(null);
  const configRef = useRef<ConfigResult | null>(null);
  const configWrites = useRef<Promise<void>>(Promise.resolve());
  // An optimistic theme toggle wins over results of writes queued before it.
  const pendingTheme = useRef<"dark" | "light" | null>(null);
  // The banner text of the last failed write, cleared by the next successful one.
  const configWriteError = useRef<string | null>(null);
  const [stats, setStats] = useState<StatsResult | null>(null);
  const [microphones, setMicrophones] = useState<MicrophoneResult[]>([]);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [runtime, setRuntime] = useState<RuntimeStatusResult | null>(null);
  const [apiKeys, setApiKeys] = useState<ApiKeyStatus>({});
  const [loading, setLoading] = useState(true);
  const [onboardingSession, setOnboardingSession] = useState(false);
  const endOnboarding = useCallback(() => setOnboardingSession(false), []);
  const [onboardingCard, setOnboardingCard] = useState(false);
  // Hiding the telemetry question without answering lasts until the next launch.
  const [telemetryQuestionHidden, setTelemetryQuestionHidden] = useState(false);
  // Release notes wait for the next launch once the introduction was shown.
  const [introductionShown, setIntroductionShown] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [recordingState, setRecordingState] = useState<RecordingState>("idle");
  const [downloadProgress, setDownloadProgress] = useState<DownloadProgress | null>(null);
  const [viewportWidth, setViewportWidth] = useState<number>(() => typeof window === "undefined" ? 1280 : window.innerWidth);
  const [permissions, setPermissions] = useState<Array<{ permission: string; hint: string; message?: string }>>([]);
  const [manualCollapse, setManualCollapse] = useState<boolean | null>(() => {
    try {
      const raw = window.localStorage.getItem(COLLAPSE_STORAGE_KEY);
      return raw === "true" ? true : raw === "false" ? false : null;
    } catch { return null; }
  });
  const accent = resolveAccent(config?.ui_accent ?? storedAccent());
  const accentMigrationStarted = useRef(false);
  useEffect(() => {
    if (!config || config.ui_accent !== undefined || accentMigrationStarted.current) return;
    accentMigrationStarted.current = true;
    void onConfigChanged({ ui_accent: storedAccent() }).then((saved) => {
      if (saved) {
        try { window.localStorage.removeItem("sotto.ui.accent"); } catch { /* optional storage */ }
      }
    });
  }, [config]);

  const autoCollapsed = viewportWidth < AUTO_COLLAPSE_BELOW;
  const collapsed = manualCollapse ?? autoCollapsed;

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    applyAccent(accent);
  }, [accent, theme]);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const onResize = () => setViewportWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  function toggleSidebarCollapse() {
    const next = !collapsed;
    setManualCollapse(next);
    try { window.localStorage.setItem(COLLAPSE_STORAGE_KEY, String(next)); } catch {/* ignore */}
  }

  useEffect(() => {
    return onRecordingStateChange(setRecordingState);
  }, []);

  async function toggleTheme() {
    const nextTheme = theme === "dark" ? "light" : "dark";
    const previousTheme = theme;
    setTheme(nextTheme);
    setConfig((current) => current ? { ...current, theme: nextTheme } : current);
    pendingTheme.current = nextTheme;
    const result = await onConfigChanged({ theme: nextTheme });
    if (pendingTheme.current === nextTheme) pendingTheme.current = null;
    if (!result) {
      setTheme(previousTheme);
      setConfig((current) => current ? { ...current, theme: previousTheme } : current);
    }
  }

  /** `onError` receives a failed write's message instead of the window banner,
   *  for callers that show the reason next to their own controls. */
  function onConfigChanged(change: ConfigChange, onError?: (message: string) => void): Promise<ConfigResult | null> {
    // Rust merges each patch with the persisted config, but full profile arrays
    // need the result of the preceding write before the next patch is formed.
    const write = async (): Promise<ConfigResult | null> => {
      try {
        const current = configRef.current;
        if (!current) return null;
        const partial = typeof change === "function" ? change(current) : change;
        const result = await invoke<ConfigResult>("save_config", { patch: partial });
        if (!result) return null;
        const stale = configWriteError.current;
        if (stale) {
          configWriteError.current = null;
          setError((shown) => (shown === stale ? null : shown));
        }
        configRef.current = result;
        setConfig(result);
        setTheme(pendingTheme.current ?? result.theme ?? "dark");
        applyLocaleFromConfig(result.ui_language);
        // Keep cross-window config notifications in the same order as writes.
        await emit("config-updated", result).catch(() => {});
        if ("model" in partial || "device" in partial) {
          invoke<ModelInfo[]>("list_models").then(setModels).catch(() => {});
        }
        return result;
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        if (onError) onError(message);
        else { configWriteError.current = message; setError(message); }
        return null;
      }
    };
    const result = configWrites.current.then(write, write);
    configWrites.current = result.then(() => {}, () => {});
    return result;
  }

  async function refreshStats() {
    const next = await getStats();
    setStats(next);
  }

  async function startOnboarding() {
    const saved = await onConfigChanged((current) => ({
      onboarding_completed: false, onboarding_step: 0, onboarding_model: current.model ?? "turbo",
    }));
    if (saved) { setOnboardingSession(true); setIntroductionShown(true); setTab("settings"); }
  }

  useEffect(() => {
    let mounted = true;
    const unlisteners: Array<() => void> = [];
    unlisteners.push(subscribe<string>("app-crash", (msg) => { if (mounted) setError(msg); }));
    // Native `app-error` events with `kind === "permission"` carry
    // a TCC-denial hint (macOS Privacy panes). We surface them as a dedicated
    // dismissable banner with a deep-link into System Settings, instead of
    // letting them get lost in the generic error line.
    unlisteners.push(subscribe<{ kind?: string; permission?: string; hint?: string; message?: string }>("app-error", (payload) => {
      if (!mounted || !payload) return;
      if (payload.kind !== "permission" || !payload.permission) return;
      if (payload.permission === "accessibility") return;
      setPermissions((current) => {
        if (current.some((p) => p.permission === payload.permission)) return current;
        return [...current, { permission: payload.permission!, hint: payload.hint ?? payload.permission!, message: payload.message }];
      });
    }));
    unlisteners.push(subscribe<ConfigResult>("config-updated", (next) => {
      if (!mounted) return;
      configRef.current = next;
      setConfig(next);
      setTheme(pendingTheme.current ?? next.theme ?? "dark");
      applyLocaleFromConfig(next.ui_language);
    }));
    // «Открыть в истории» on the overlay's late-answer note.
    unlisteners.push(subscribe<number>("open-history-entry", (id) => {
      if (!mounted || typeof id !== "number") return;
      setHistoryFocus((current) => ({ id, seq: (current?.seq ?? 0) + 1 }));
      setTab("history");
    }));
    // `paste-done`, not `whisper-done`: stats and the history row are
    // written after the LLM pass, so refreshing on decode read the numbers
    // from before this transcription was recorded.
    unlisteners.push(subscribe<unknown>("paste-done", () => {
      if (mounted) void refreshStats().catch(() => {});
    }));
    const refreshModels = () => {
      invoke<ModelInfo[]>("list_models").then((next) => { if (mounted) setModels(next); }).catch(() => {});
    };
    const refreshRuntime = () => {
      invoke<RuntimeStatusResult>("get_runtime_status").then((next) => { if (mounted) setRuntime(next); }).catch(() => {});
    };
    unlisteners.push(subscribe<unknown>("model-ready", () => {
      if (mounted) setDownloadProgress(null);
      refreshModels();
    }));
    unlisteners.push(subscribe<string>("whisper-loading", (name) => {
      if (!mounted) return;
      setRecordingState("loading");
      setRuntime((current) => ({
        ...current,
        model_loaded: false,
        model_loads_on_demand: false,
        model: name,
        loaded_model: null,
        device: null,
        engine: current?.engine ?? null,
        cpu_only: false,
        recording: current?.recording ?? false,
        state: "loading",
      }));
    }));
    unlisteners.push(subscribe<string>("whisper-ready", () => {
      if (!mounted) return;
      setDownloadProgress(null);
      refreshModels();
      refreshRuntime();
      setRecordingState((current) => current === "loading" ? "idle" : current);
    }));
    unlisteners.push(subscribe<{ name?: string; message?: string }>("whisper-load-failed", (payload) => {
      if (!mounted) return;
      setDownloadProgress(null);
      refreshRuntime();
      setRecordingState("error");
      if (payload?.message) setError(payload.message);
    }));
    // Unloading clears both the loaded model and its flag in the list — we
    // refresh both slices, otherwise the sidebar status stays on a deleted one.
    unlisteners.push(subscribe<unknown>("model-unloaded", () => { refreshModels(); refreshRuntime(); }));
    // The model returning after an idle unload. Deliberately separate from
    // `whisper-ready`: that one drives the dictation state, while this arrives
    // in the middle of somebody else's recording and has no business touching
    // that state — only the lists.
    unlisteners.push(subscribe<unknown>("model-restored", () => { refreshModels(); refreshRuntime(); }));
    unlisteners.push(subscribe<DownloadProgress>("model-download-progress", (payload) => {
      if (!mounted || !payload) return;
      setDownloadProgress({
        model: payload.model,
        downloaded: Number(payload.downloaded) || 0,
        total: typeof payload.total === "number" ? payload.total : null,
      });
    }));

    async function loadApiKeys(cfg: ConfigResult | null): Promise<ApiKeyStatus> {
      const defaults = ["anthropic", "openai", "gemini", "opencode-go", "compatible"];
      const profileRefs = (cfg?.ai_processing?.profiles ?? [])
        .map((p) => p.api_key_ref || (p.id === "default" ? p.provider : `key_${p.id}`))
        .filter(Boolean);
      // Profile-less slots: the OS store cannot be enumerated and `has_api_key`
      // can only answer about a known ref. Without asking about them here we
      // lose the key from the UI on every restart.
      const slotRefs = (cfg?.ai_processing?.key_slots ?? []).map((s) => s.ref).filter(Boolean);
      const ids = Array.from(new Set([...defaults, ...profileRefs, ...slotRefs]));
      const entries = await Promise.all(ids.map(async (key_id) => {
        try {
          const result = await invoke<{ available: boolean; label?: string; masked?: string }>("has_api_key", { key_id });
          return [key_id, { available: !!result.available, label: result.label ?? "", masked: result.masked ?? "" }] as const;
        } catch {
          return [key_id, { available: false, label: "", masked: "" }] as const;
        }
      }));
      return Object.fromEntries(entries) as ApiKeyStatus;
    }

    async function load() {
      try {
        const setters: Array<{ p: Promise<unknown>; set: (v: unknown) => void; name: string }> = [
          { p: invoke<AppVersionResult>("app_version"), set: (v) => { if (mounted) setVersion((v as AppVersionResult).version); }, name: "app_version" },
          { p: invoke<ConfigResult>("get_config"), set: (v) => { if (mounted) { const cfg = v as ConfigResult; configRef.current = cfg; setConfig(cfg); setOnboardingSession(cfg.onboarding_completed === false); setIntroductionShown(cfg.onboarding_completed === false); setTheme(cfg.theme ?? "dark"); applyLocaleFromConfig(cfg.ui_language); } }, name: "get_config" },
          { p: invoke<MicrophoneResult[]>("list_microphones"), set: (v) => { if (mounted) setMicrophones(v as MicrophoneResult[]); }, name: "list_microphones" },
          { p: invoke<ModelInfo[]>("list_models"), set: (v) => { if (mounted) setModels(v as ModelInfo[]); }, name: "list_models" },
          { p: getStats(), set: (v) => { if (mounted) setStats(v as StatsResult); }, name: "get_stats" },
          { p: invoke<RuntimeStatusResult>("get_runtime_status"), set: (v) => { if (mounted) setRuntime(v as RuntimeStatusResult); }, name: "get_runtime_status" },
        ];
        const results = await Promise.allSettled(setters.map((s) => s.p));
        // Every failure here is visible to the user. It used to go only to
        // console.warn, and a release build has no DevTools — the app silently
        // drew an empty config as "there are no settings yet", with no way to
        // tell that apart from an honest first launch.
        const failed: string[] = [];
        results.forEach((r, i) => {
          if (r.status === "fulfilled") {
            setters[i].set(r.value);
            return;
          }
          const reason = r.reason instanceof Error ? r.reason.message : String(r.reason);
          console.warn("[MainWindow] load:", setters[i].name, "failed", r.reason);
          failed.push(`${setters[i].name}: ${reason}`);
        });
        if (failed.length > 0 && mounted) {
          setError(t("Не загрузилось при старте — {p0}", { p0: failed.join("; ") }));
        }
        // Determine which config we actually got (or fall back to null)
        const appConfig = results[1].status === "fulfilled" ? results[1].value as ConfigResult : null;
        if (mounted && staleBuiltInPrompt(appConfig?.ai_processing)) {
          void onConfigChanged((current) => {
            const system_prompt = staleBuiltInPrompt(current.ai_processing);
            return system_prompt ? { ai_processing: { system_prompt } } : {};
          });
        }
        const keyStatuses = await loadApiKeys(appConfig);
        if (!mounted) return;
        setApiKeys(keyStatuses);
      } catch (e) {
        if (mounted) setError(e instanceof Error ? e.message : String(e));
      } finally {
        if (mounted) setLoading(false);
      }
    }

    load();
    return () => {
      mounted = false;
      unlisteners.forEach((stop) => stop());
    };
  }, []);

  // A mirror of the Rust gate `transcription_route_available`: while there is
  // nothing to transcribe with, the hotkey silently does nothing — an overlay
  // for a recording that will produce no text would be a lie. The pill explains
  // the silence and offers both ways out: download a model or move to the cloud.
  const pipelineMode = config?.ai_processing?.pipeline_mode ?? "local";
  const selectedModel = models.find((item) => item.selected) ?? models.find((item) => item.id === config?.model);
  const sttUnavailable = models.length > 0
    && pipelineMode !== "cloud"
    && !runtime?.loaded_model?.trim()
    && !selectedModel?.downloaded;
  const onboardingActive = config?.onboarding_completed === false;
  const askTelemetry = !loading && !onboardingCard && !telemetryQuestionHidden
    && shouldOfferTelemetryConsent(config, stats?.total_transcriptions ?? 0);

  return (
    <div className="app-frame" style={{ width: "100%", height: "100%", padding: 0 }}>
      {!loading && !introductionShown && !onboardingActive && <WhatsNewDialog ready={recordingState === "idle"}/>}
      {!loading && config && !onboardingActive && <UpdateNotice
        receiveBeta={config.receive_beta_updates === true}
        ready={recordingState === "idle" && tab !== "info"}
        lastShownAt={config.update_reminder_shown_at}
        onShown={(timestamp) => { void onConfigChanged({ update_reminder_shown_at: timestamp }, () => {}); }}
        onUpdate={() => { setUpdateInstallRequested(true); setTab("info"); }}
      />}
      <div className={`win${collapsed && !onboardingActive ? " collapsed" : ""}`}>
        <TitleBar actionsRef={setOnboardingToolbar} collapsed={collapsed && !onboardingActive} fullWidth={onboardingActive} onToggleCollapse={onboardingActive ? undefined : toggleSidebarCollapse}/>
        <div className={`win__layout${onboardingActive ? " win__layout--onboarding" : collapsed ? " collapsed" : ""}`}>
          {!onboardingActive && <Sidebar tab={tab} onTab={setTab} recordingState={recordingState} pipelineMode={config?.ai_processing?.pipeline_mode} loadedModel={actualModelLabel(runtime, "")} loadsOnDemand={runtime?.model_loads_on_demand} theme={theme} onToggleTheme={() => void toggleTheme()} downloadProgress={downloadProgress} collapsed={collapsed}/>}
          <main className="win__main" data-testid="main-content">
            {!loading && !onboardingActive && <AccessibilityNotice/>}
            {permissions.length > 0 && permissions.map((p) => (
              <Card key={p.permission} pad="rows" className="window-banner window-banner--stacked permission-notice">
                <div className="window-banner__copy" role="alert">
                  <strong className="permission-notice__title"><Icon name="info" size={16}/>{t("Нужно разрешение macOS —")} {p.hint}.</strong>
                  <p>{p.message ? `${p.message} ` : ""}{t("После выдачи прав перезапустите приложение.")}</p>
                </div>
                <div className="window-banner__actions">
                  <button className="btn btn--primary" type="button" onClick={() => openPrivacyPane(p.permission)}>{t("Открыть настройки macOS")}</button>
                </div>
                <button className="btn btn--ghost btn--icon window-banner__close" type="button" aria-label={t("Закрыть")} onClick={() => setPermissions((cur) => cur.filter((q) => q.permission !== p.permission))}>
                  <Icon name="x" size={14}/>
                </button>
              </Card>
            ))}
            {error && <div role="alert" style={{ margin: "14px 32px 0", padding: "10px 12px", borderRadius: "var(--radius-sm)", background: "var(--err-soft)", border: "1px solid color-mix(in srgb, var(--err) 35%, transparent)", color: "var(--err)", font: "500 12px/1.35 var(--font-sans)" }}>{error}</div>}
            {sttUnavailable && !onboardingCard && !onboardingActive && (
              <div role="status" style={{ margin: "14px 32px 0", display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap", padding: "12px 14px", borderRadius: "var(--radius-sm)", background: "var(--warn-soft)", border: "1px solid color-mix(in srgb, var(--warn) 30%, transparent)", color: "var(--warn)", font: "500 12.5px/1.4 var(--font-sans)" }}>
                <Icon name="info" size={14}/>
                <span style={{ flex: "1 1 240px", minWidth: 240 }}>
                  <strong>{t("Модель распознавания не скачана.")}</strong>  {t("Для записи скачайте модель распознавания в разделе «Модели».")} </span>
                <button className="btn btn--primary" type="button" onClick={() => setTab("models")} style={{ height: 28 }}>
                  <Icon name="settings" size={12}/>  {t("Скачать модель")} </button>
              </div>
            )}
            {askTelemetry && <TelemetryConsentCard onConfigChanged={onConfigChanged} onDismiss={() => setTelemetryQuestionHidden(true)}/>}
            {(onboardingSession || onboardingActive) && config && !loading && <Suspense fallback={null}>
              <Onboarding toolbarTarget={onboardingToolbar} active={onboardingActive} config={config} models={models} microphones={microphones} runtime={runtime} progress={downloadProgress} onConfigChanged={onConfigChanged} onModelsChanged={setModels} onNavigate={setTab} onToggleTheme={() => void toggleTheme()} onCardShown={setOnboardingCard} onEnd={endOnboarding}/>
            </Suspense>}
            {loading ? <LoadingState/> : !onboardingActive && (
              <div data-testid={`page-${tab}`} style={{ position: "relative", flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
                {pageFor(tab, { config, historyFocus, version, stats, microphones, models, runtime, apiKeys, onConfigChanged, textPreviewDraft, onTextPreviewDraftChange: setTextPreviewDraft, onNavigate: setTab, onApiKeysChanged: setApiKeys, onModelsChanged: setModels, onStatsRefresh: refreshStats, onStartOnboarding: () => void startOnboarding(), updateInstallRequested, onUpdateInstallRequestHandled: () => setUpdateInstallRequested(false) })}
              </div>
            )}
          </main>
        </div>
      </div>
    </div>
  );
}

function LoadingState() {
  return <div className="loading-state" data-testid="startup-loading"><div style={{ display: "flex", alignItems: "center", gap: 10, font: "500 13px/1 var(--font-sans)" }}><div style={{ width: 16, height: 16, borderRadius: "50%", border: "2px solid var(--bg-4)", borderTopColor: "var(--accent)", animation: "spin .8s linear infinite" }}/>{t("Подключение к backend")}</div></div>;
}
