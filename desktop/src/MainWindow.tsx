import { useEffect, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { invoke, on, onRecordingStateChange, type RecordingState, waitForReady } from "./bridge";
import { getStats } from "./bridge/stats";
import type { ApiKeyStatus, AppVersionResult, ConfigResult, MicrophoneResult, ModelInfo, RuntimeStatusResult, StatsResult } from "./bridge/types";
import { Sidebar, TitleBar, ACCENT_OPTIONS, applyAccent, type TabId, type DownloadProgress, type AccentValue } from "./components/Shell";
import { Icon } from "./components/Icon";

const ACCENT_STORAGE_KEY = "sotto.ui.accent";
const COLLAPSE_STORAGE_KEY = "sotto.ui.sidebarCollapsed";
const AUTO_COLLAPSE_BELOW = 1100;
import { SettingsPage } from "./pages/SettingsPage";
import { ModelsPage } from "./pages/ModelsPage";
import { AiPage } from "./pages/AiPage";
import { IntegrationsPage } from "./pages/IntegrationsPage";
import { HistoryPage } from "./pages/HistoryPage";
import { InfoPage, StatsPage, TextPage } from "./pages/OtherPages";
import { actualModelLabel } from "./pages/runtimePresentation";
import { applyLocaleFromConfig, t, useLocale } from "./i18n";

const MVP_TABS: TabId[] = ["settings", "models", "text", "ai", "integrations", "history", "stats", "info"];

function isMvpTab(tab: TabId) {
  return MVP_TABS.includes(tab);
}

// macOS URL schemes that deep-link into Privacy & Security panes. Opening one
// via System Settings' `x-apple.systempreferences:` handler lands the user on
// the exact section where they can grant Microphone / Accessibility to the
// process running the sidecar (Python.app in dev, the app bundle in prod).
const PRIVACY_URLS: Record<string, string> = {
  microphone: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
  accessibility: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
};

function openPrivacyPane(permission: string) {
  const url = PRIVACY_URLS[permission] ?? "x-apple.systempreferences:com.apple.preference.security?Privacy";
  // The Rust side `open_url` shells out to `open`/`xdg-open`/`start`.
  invoke<null>("open_url", { url }).catch(() => {/* ignore */});
}

function pageFor(tab: TabId, data: {
  config: ConfigResult | null;
  version: string | null;
  stats: StatsResult | null;
  microphones: MicrophoneResult[];
  models: ModelInfo[];
  apiKeys: ApiKeyStatus;
  onConfigChanged: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null>;
  onNavigate: (tab: TabId) => void;
  onApiKeysChanged: (next: ApiKeyStatus) => void;
  onModelsChanged: (models: ModelInfo[]) => void;
  onStatsRefresh: () => Promise<void>;
}) {
  switch (tab) {
    case "settings": return <SettingsPage config={data.config} microphones={data.microphones} models={data.models} onConfigChanged={data.onConfigChanged}/>;
    case "models": return <ModelsPage models={data.models} config={data.config} onConfigChanged={data.onConfigChanged} onModelsChanged={data.onModelsChanged}/>;
    case "text": return <TextPage config={data.config} onConfigChanged={data.onConfigChanged}/>;
    case "ai": return <AiPage config={data.config?.ai_processing ?? null} apiKeys={data.apiKeys} onConfigChanged={data.onConfigChanged} onNavigate={(t) => data.onNavigate(t)}/>;
    case "integrations": return <IntegrationsPage config={data.config?.ai_processing ?? null} apiKeys={data.apiKeys} onConfigChanged={data.onConfigChanged} onApiKeysChanged={data.onApiKeysChanged}/>;
    case "history": return <HistoryPage/>;
    case "stats": return <StatsPage stats={data.stats} typingSpeedCpm={data.config?.typing_speed_cpm} onRefresh={data.onStatsRefresh}/>;
    case "info": return <InfoPage version={data.version} config={data.config} onConfigChanged={data.onConfigChanged}/>;
  }
}

export function MainWindow() {
  // Одна подписка на язык в корне: t() читает модульное состояние, так что
  // перерисовки корня достаточно для всего дерева.
  useLocale();
  const [tab, setTab] = useState<TabId>("settings");
  const [theme, setTheme] = useState<"dark" | "light">("dark");
  const [version, setVersion] = useState<string | null>(null);
  const [config, setConfig] = useState<ConfigResult | null>(null);
  const [stats, setStats] = useState<StatsResult | null>(null);
  const [microphones, setMicrophones] = useState<MicrophoneResult[]>([]);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [runtime, setRuntime] = useState<RuntimeStatusResult | null>(null);
  const [apiKeys, setApiKeys] = useState<ApiKeyStatus>({});
  const [loading, setLoading] = useState(true);
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
  const [accent] = useState<AccentValue>(() => {
    try {
      const raw = window.localStorage.getItem(ACCENT_STORAGE_KEY);
      const known = ACCENT_OPTIONS().find((o) => o.value.toLowerCase() === (raw ?? "").toLowerCase());
      return (known?.value ?? ACCENT_OPTIONS()[0].value) as AccentValue;
    } catch { return ACCENT_OPTIONS()[0].value as AccentValue; }
  });

  const autoCollapsed = viewportWidth < AUTO_COLLAPSE_BELOW;
  const collapsed = manualCollapse ?? autoCollapsed;

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    applyAccent(accent);
    try { window.localStorage.setItem(ACCENT_STORAGE_KEY, accent); } catch {/* ignore */}
  }, [accent]);

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
    try {
      const result = await invoke<ConfigResult>("save_config", { patch: { theme: nextTheme } });
      if (result) { setConfig(result); void emit("config-updated", result).catch(() => {}); }
    } catch (e) {
      setTheme(previousTheme);
      setConfig((current) => current ? { ...current, theme: previousTheme } : current);
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function onConfigChanged(partial: Partial<ConfigResult>): Promise<ConfigResult | null> {
    try {
      const result = await invoke<ConfigResult>("save_config", { patch: partial });
      if (result) {
        setConfig(result);
        void emit("config-updated", result).catch(() => {});
        if ("model" in partial || "device" in partial) {
          invoke<ModelInfo[]>("list_models").then(setModels).catch(() => {});
        }
        return result;
      }
      return null;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return null;
    }
  }

  async function refreshStats() {
    const next = await getStats();
    setStats(next);
  }

  useEffect(() => {
    let mounted = true;
    let unlistenCrash: (() => void) | null = null;
    let unlistenModelLoaded: (() => void) | null = null;
    let unlistenWhisperReady: (() => void) | null = null;
    let unlistenModelUnloaded: (() => void) | null = null;
    let unlistenModelRestored: (() => void) | null = null;
    let unlistenModelLoading: (() => void) | null = null;
    let unlistenModelFailed: (() => void) | null = null;
    let unlistenNavigate: (() => void) | null = null;
    let unlistenConfigUpdated: (() => void) | null = null;
    let unlistenTranscriptionDone: (() => void) | null = null;
    on<string>("app-crash", (msg) => { if (mounted) setError(msg); }).then((fn) => { unlistenCrash = fn; });
    let unlistenPermission: (() => void) | null = null;
    // Structured sidecar ``error`` events with ``kind === "permission"`` carry
    // a TCC-denial hint (macOS Privacy panes). We surface them as a dedicated
    // dismissable banner with a deep-link into System Settings, instead of
    // letting them get lost in the generic error line.
    on<{ kind?: string; permission?: string; hint?: string; message?: string }>("app-error", (payload) => {
      if (!mounted || !payload) return;
      if (payload.kind !== "permission" || !payload.permission) return;
      setPermissions((current) => {
        if (current.some((p) => p.permission === payload.permission)) return current;
        return [...current, { permission: payload.permission!, hint: payload.hint ?? payload.permission!, message: payload.message }];
      });
    }).then((fn) => { unlistenPermission = fn; });
    on<string>("navigate-tab", (next) => {
      // «Форматирование» + «Замены» слились в «Текст», «Провайдеры» +
      // «API-ключи» — в «Интеграции», «Обзор» убран целиком. Псевдонимы
      // оставлены, потому что событие шлёт трей: отдельное окно, которое
      // может остаться от предыдущей версии сборки и знать только старые
      // идентификаторы.
      const legacy: Record<string, TabId> = { formatting: "text", replacements: "text", providers: "integrations", "api-keys": "integrations", overview: "settings" };
      const resolved = (legacy[next] ?? next) as TabId;
      if (mounted && MVP_TABS.includes(resolved)) setTab(resolved);
    }).then((fn) => { unlistenNavigate = fn; });
    on<ConfigResult>("config-updated", (next) => {
      if (!mounted) return;
      setConfig(next);
      setTheme(next.theme ?? "dark");
      applyLocaleFromConfig(next.ui_language);
    }).then((fn) => { unlistenConfigUpdated = fn; });
    // `paste-done`, not `whisper-done`: stats and the history row are
    // written after the LLM pass, so refreshing on decode read the numbers
    // from before this transcription was recorded.
    on<unknown>("paste-done", () => {
      if (mounted) void refreshStats().catch(() => {});
    }).then((fn) => { unlistenTranscriptionDone = fn; });
    const refreshModels = () => {
      invoke<ModelInfo[]>("list_models").then((next) => { if (mounted) setModels(next); }).catch(() => {});
    };
    const refreshRuntime = () => {
      invoke<RuntimeStatusResult>("get_runtime_status").then((next) => { if (mounted) setRuntime(next); }).catch(() => {});
    };
    let unlistenDownloadProgress: (() => void) | null = null;
    on<unknown>("model-ready", () => {
      if (mounted) setDownloadProgress(null);
      refreshModels();
    }).then((fn) => { unlistenModelLoaded = fn; });
    on<string>("whisper-loading", (name) => {
      if (!mounted) return;
      setRecordingState("loading");
      setRuntime((current) => ({
        model_loaded: false,
        model: name,
        loaded_model: null,
        device: null,
        engine: current?.engine ?? null,
        cpu_only: false,
        recording: current?.recording ?? false,
        state: "loading",
        last_error: null,
      }));
    }).then((fn) => { unlistenModelLoading = fn; });
    on<string>("whisper-ready", () => {
      if (!mounted) return;
      setDownloadProgress(null);
      refreshModels();
      refreshRuntime();
      setRecordingState((current) => current === "loading" ? "idle" : current);
    }).then((fn) => { unlistenWhisperReady = fn; });
    on<{ name?: string; message?: string }>("whisper-load-failed", (payload) => {
      if (!mounted) return;
      setDownloadProgress(null);
      refreshRuntime();
      setRecordingState("error");
      if (payload?.message) setError(payload.message);
    }).then((fn) => { unlistenModelFailed = fn; });
    // Выгрузка снимает и загруженную модель, и её признак в списке —
    // обновляем оба среза, иначе статус в сайдбаре остаётся на удалённой.
    on<unknown>("model-unloaded", () => { refreshModels(); refreshRuntime(); }).then((fn) => { unlistenModelUnloaded = fn; });
    // Возврат модели после выгрузки по простою. Отдельно от `whisper-ready`
    // намеренно: тот ведёт состояние диктовки, а этот приходит посреди
    // чужой записи, и трогать её состояние ему нечем — только списки.
    on<unknown>("model-restored", () => { refreshModels(); refreshRuntime(); }).then((fn) => { unlistenModelRestored = fn; });
    on<DownloadProgress>("model-download-progress", (payload) => {
      if (!mounted || !payload) return;
      setDownloadProgress({
        model: payload.model,
        downloaded: Number(payload.downloaded) || 0,
        total: typeof payload.total === "number" ? payload.total : null,
      });
    }).then((fn) => { unlistenDownloadProgress = fn; });

    async function loadApiKeys(cfg: ConfigResult | null): Promise<ApiKeyStatus> {
      const defaults = ["anthropic", "openai", "gemini", "opencode-go", "compatible"];
      const profileRefs = (cfg?.ai_processing?.profiles ?? [])
        .map((p) => p.api_key_ref || (p.id === "default" ? p.provider : `key_${p.id}`))
        .filter(Boolean);
      // Слоты без профиля: хранилище ОС не перечисляется, `has_api_key` умеет
      // ответить только про известный ref. Не спросив о них здесь, мы теряем
      // ключ из интерфейса при каждом перезапуске.
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
        await waitForReady();
        const setters: Array<{ p: Promise<unknown>; set: (v: unknown) => void; name: string }> = [
          { p: invoke<AppVersionResult>("app_version"), set: (v) => { if (mounted) setVersion((v as AppVersionResult).version); }, name: "app_version" },
          { p: invoke<ConfigResult>("get_config"), set: (v) => { if (mounted) { const cfg = v as ConfigResult; setConfig(cfg); setTheme(cfg.theme ?? "dark"); applyLocaleFromConfig(cfg.ui_language); } }, name: "get_config" },
          { p: invoke<MicrophoneResult[]>("list_microphones"), set: (v) => { if (mounted) setMicrophones(v as MicrophoneResult[]); }, name: "list_microphones" },
          { p: invoke<ModelInfo[]>("list_models"), set: (v) => { if (mounted) setModels(v as ModelInfo[]); }, name: "list_models" },
          { p: getStats(), set: (v) => { if (mounted) setStats(v as StatsResult); }, name: "get_stats" },
          { p: invoke<RuntimeStatusResult>("get_runtime_status"), set: (v) => { if (mounted) setRuntime(v as RuntimeStatusResult); }, name: "get_runtime_status" },
        ];
        const results = await Promise.allSettled(setters.map((s) => s.p));
        // Каждый отказ здесь виден пользователю. Раньше он уходил только в
        // console.warn, а в релизной сборке DevTools нет — приложение молча
        // рисовало пустой конфиг как «настроек ещё нет», и отличить это от
        // честного первого запуска было нечем.
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
      unlistenCrash?.();
      unlistenPermission?.();
      unlistenModelLoaded?.();
      unlistenWhisperReady?.();
      unlistenModelLoading?.();
      unlistenModelFailed?.();
      unlistenModelUnloaded?.();
      unlistenModelRestored?.();
      unlistenDownloadProgress?.();
      unlistenNavigate?.();
      unlistenConfigUpdated?.();
      unlistenTranscriptionDone?.();
    };
  }, []);

  // Зеркало rust-гейта `transcription_route_available`: пока распознавать
  // нечем, горячая клавиша молча ничего не делает — оверлей на запись, из
  // которой не выйдет текста, врал бы. Плашка объясняет молчание и даёт оба
  // выхода: скачать модель или уйти в облако.
  const pipelineMode = config?.ai_processing?.pipeline_mode ?? "local";
  const selectedModel = models.find((item) => item.selected) ?? models.find((item) => item.id === config?.model);
  const sttUnavailable = models.length > 0
    && pipelineMode !== "cloud"
    && !runtime?.loaded_model?.trim()
    && !selectedModel?.downloaded;

  return (
    <div className="app-frame" style={{ width: "100%", height: "100%", padding: 0 }}>
      <div className={`win${collapsed ? " collapsed" : ""}`}>
        <TitleBar collapsed={collapsed} onToggleCollapse={toggleSidebarCollapse}/>
        <div className={`win__layout${collapsed ? " collapsed" : ""}`}>
          <Sidebar tab={tab} onTab={setTab} recordingState={recordingState} pipelineMode={config?.ai_processing?.pipeline_mode} loadedModel={actualModelLabel(runtime, "")} loadsOnDemand={runtime?.model_loads_on_demand} theme={theme} onToggleTheme={() => void toggleTheme()} downloadProgress={downloadProgress} collapsed={collapsed}/>
          <main className="win__main">
            {permissions.length > 0 && permissions.map((p) => (
              <div key={p.permission} role="alert" style={{ margin: "14px 32px 0", display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap", padding: "12px 14px", borderRadius: 8, background: "var(--accent-soft)", border: "1px solid var(--accent-line)", color: "var(--accent-text)", font: "500 12.5px/1.4 var(--font-sans)" }}>
                <Icon name="info" size={14}/>
                <span style={{ flex: "1 1 240px", minWidth: 240 }}>
                  <strong>{t("Нужно разрешение macOS —")} {p.hint}.</strong>
                  {p.message ? ` ${p.message}` : ""}
                  {" "}{t("После выдачи прав перезапустите приложение.")} </span>
                <button className="btn btn--primary" type="button" onClick={() => openPrivacyPane(p.permission)} style={{ height: 28 }}>
                  <Icon name="globe" size={12}/>  {t("Открыть System Settings")} </button>
                <button className="btn btn--ghost" type="button" aria-label={t("Закрыть")} onClick={() => setPermissions((cur) => cur.filter((q) => q.permission !== p.permission))} style={{ height: 28, lineHeight: 1 }}>
                  <Icon name="x" size={12}/>
                </button>
              </div>
            ))}
            {error && <div role="alert" style={{ margin: "14px 32px 0", padding: "10px 12px", borderRadius: 8, background: "rgba(239,94,107,0.12)", border: "1px solid rgba(239,94,107,0.35)", color: "var(--err)", font: "500 12px/1.35 var(--font-sans)" }}>{error}</div>}
            {sttUnavailable && (
              <div role="status" style={{ margin: "14px 32px 0", display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap", padding: "12px 14px", borderRadius: 8, background: "var(--warn-soft)", border: "1px solid rgba(251,191,36,0.30)", color: "var(--warn)", font: "500 12.5px/1.4 var(--font-sans)" }}>
                <Icon name="info" size={14}/>
                <span style={{ flex: "1 1 240px", minWidth: 240 }}>
                  <strong>{t("Модель распознавания не скачана.")}</strong>  {t("Для записи скачайте модель распознавания в разделе «Модели».")} </span>
                <button className="btn btn--primary" type="button" onClick={() => setTab("models")} style={{ height: 28 }}>
                  <Icon name="settings" size={12}/>  {t("Скачать модель")} </button>
              </div>
            )}
            {loading ? <LoadingState/> : (
              <PageWithMvpGate tab={tab}>
                {isMvpTab(tab)
                  ? pageFor(tab, { config, version, stats, microphones, models, apiKeys, onConfigChanged, onNavigate: setTab, onApiKeysChanged: setApiKeys, onModelsChanged: setModels, onStatsRefresh: refreshStats })
                  : <DeferredPage tab={tab}/>}
              </PageWithMvpGate>
            )}
          </main>
        </div>
      </div>
    </div>
  );
}

function PageWithMvpGate({ tab, children }: { tab: TabId; children: React.ReactNode }) {
  const isMvpReady = isMvpTab(tab);
  return (
    <div style={{ position: "relative", flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
      {children}
      {!isMvpReady && (
        <div
          aria-hidden="true"
          style={{
            position: "absolute",
            inset: 0,
            zIndex: 50,
            display: "grid",
            placeItems: "center",
            cursor: "not-allowed",
            background: "rgba(128, 128, 128, 0.34)",
            backdropFilter: "blur(1.5px) saturate(80%)",
            WebkitBackdropFilter: "blur(1.5px) saturate(80%)",
          }}
        >
          <span
            className="tag"
            style={{
              height: 32,
              padding: "0 16px",
              fontSize: 12,
              background: "var(--surface-3)",
              color: "var(--text-mute)",
              borderColor: "var(--border-strong)",
              boxShadow: "var(--shadow-2)",
            }}
          >
             {t("В доработке")} </span>
        </div>
      )}
    </div>
  );
}

function DeferredPage({ tab }: { tab: TabId }) {
  const labels: Record<TabId, { title: string; detail: string }> = {
    settings: { title: t("Настройки"), detail: "" },
    models: { title: t("Модели"), detail: "" },
    text: { title: t("Текст"), detail: "" },
    ai: { title: t("LLM-обработка"), detail: "" },
    integrations: { title: t("Провайдеры и ключи"), detail: "" },
    history: { title: t("История"), detail: "" },
    stats: { title: t("Статистика"), detail: t("Раздел статистики загружается как часть MVP.") },
    info: { title: t("Справка"), detail: t("Справочный раздел будет подключен позже.") },
  };
  const item = labels[tab];
  return (
    <>
      <header className="main-header">
        <div>
          <h1>{item.title}</h1>
          <p>{item.detail}</p>
        </div>
      </header>
      <div className="main-body">
        <div className="card" style={{ padding: 18, color: "var(--text-2)", font: "500 13px/1.45 var(--font-sans)" }}>
           {t("Раздел не монтируется в MVP, поэтому связанные команды backend не вызываются.")} </div>
      </div>
    </>
  );
}

function LoadingState() {
  return <div className="loading-state"><div style={{ display: "flex", alignItems: "center", gap: 10, font: "500 13px/1 var(--font-sans)" }}><div style={{ width: 16, height: 16, borderRadius: "50%", border: "2px solid var(--surface-3)", borderTopColor: "var(--accent)", animation: "spin .8s linear infinite" }}/>{t("Подключение к backend")}</div></div>;
}
