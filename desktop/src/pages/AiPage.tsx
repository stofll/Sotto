import { useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { invoke, on } from "../bridge";
import { PageHeader, SettingRow } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Hint } from "../components/Hint";
import {
  activeConfigFromProfile,
  effectiveSystemPrompt,
  mergeAi,
  presetPrompt,
  promptIsCustom,
  normalizeProfile,
  profileKeyRef,
  llmRouteBlocker,
  profilesForAi,
  PROVIDERS,
  SYSTEM_PROMPT_PRESETS,
  textProfileFor,
  type AiConfig,
  type LlmRouteBlocker,
} from "./aiShared";
import { CustomSelect } from "../components/CustomSelect";
import { NumberField } from "../components/NumberField";
import type { ApiKeyInfo, ApiKeyStatus, ConfigResult } from "../bridge/types";
import { t } from "../i18n";

type AiRunResult = {
  available: boolean;
  output?: string;
  message?: string;
  fallback?: boolean;
  provider_error?: string;
  skipped_reason?: string;
  http_status?: number;
  /** The first 500 characters of the response body. The backend was sending it
   *  before, but the field was not declared here, and the only source of truth
   *  about "the response has the wrong shape" was silently lost between Rust
   *  and the screen. */
  response_snippet?: string;
  ai_processing?: { attempted?: boolean; used?: boolean; skipped_reason?: string };
};

/** The processing stage of an attached file. The engine reports no progress, so
 *  all we can show is which of the three steps we are on. */
type FileStage = null | "decoding" | "transcribing";

/** The response of `transcribe_audio_file`. A different shape from
 *  `AiRunResult`: that is the result of one LLM call, this is the entire
 *  recognition pipeline. */
type TranscribeFileResult = {
  text: string;
  raw_text: string;
  formatted_text: string;
  ai_status: {
    used?: boolean;
    fallback?: boolean;
    attempted?: boolean;
    skipped_reason?: string;
  } | null;
  audio_seconds: number;
  inference_time_ms: number;
  language: string | null;
};

/** The status pill for a file result.
 *
 *  Separate from the «Обработать текст» pills: there `available === false`
 *  means "the LLM did not run", and that is an error. Here the LLM may
 *  legitimately not run — in local-only mode it should not — while the text is
 *  transcribed all the same. A red pill on a successful transcription would be
 *  a lie. */
function FileStatusPill({ result }: { result: TranscribeFileResult }) {
  const ai = result.ai_status;
  if (ai?.fallback) return <span className="pill warn">Fallback</span>;
  if (ai?.used) return <span className="pill ok">{t("Готово")}</span>;
  if (ai?.attempted) return <span className="pill warn">{t("LLM не отработала")}</span>;
  return <span className="pill">{t("Распознано")}</span>;
}

/** The provider's raw response beneath the error. Collapsed: needed when the
 *  error message says "wrong response shape", and unnecessary in every other
 *  case. */
function ProviderSnippet({ result }: { result: AiRunResult }) {
  if (!result.response_snippet) return null;
  return (
    <details style={{ marginTop: 2 }}>
      <summary style={{ cursor: "pointer", font: "500 11px/1.4 var(--font-sans)", color: "var(--ink-mute)" }}>
        {result.http_status ? t("Ответ провайдера (HTTP {p0})", { p0: result.http_status }) : t("Ответ провайдера")}
      </summary>
      <pre className="scroll-visible" style={{ margin: "6px 0 0", padding: 10, maxHeight: 180, overflow: "auto", borderRadius: "var(--r-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 11px/1.5 var(--font-mono)", color: "var(--ink-mute)", whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
        {result.response_snippet}
      </pre>
    </details>
  );
}

const PIPELINE_MODES = () => ([
  { id: "local", title: t("Только локально"), sub: t("STT на этом компьютере. LLM не вызывается."), icon: "shield" },
  { id: "hybrid", title: t("Локальное распознавание + LLM"), sub: t("Распознаем локально, затем отправляем текст в LLM для обработки."), icon: "wand" },
  { id: "cloud", title: t("Облачное распознавание"), sub: t("Аудио уходит на OpenAI-совместимый эндпоинт /audio/transcriptions. Нужны Base URL, модель и ключ активного профиля."), icon: "spark" },
] as const);

/** What exactly stops the chosen mode from reaching the provider. */
function blockerReason(blocker: NonNullable<LlmRouteBlocker>): string {
  if (blocker === "no_provider") return t("Провайдер LLM не выбран. Настройте его в «Интеграциях».");
  if (blocker === "invalid_base_url") return t("Для облачного распознавания укажите Base URL с http:// или https://.");
  if (blocker === "no_model") return t("Модель обработки не выбрана.");
  return t("Для обработки нет сохранённого API-ключа.");
}

const EMPTY_KEY_INFO: ApiKeyInfo = { available: false, label: "", masked: "" };

type Props = {
  config: AiConfig | null;
  apiKeys: ApiKeyStatus;
  onConfigChanged: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null>;
  onNavigate: (tab: "integrations") => void;
};

/** Exactly the same list as in the `pick_audio_file` filter on the Rust side. */
const AUDIO_EXTENSIONS = ["wav", "mp3", "m4a", "mp4", "ogg", "oga", "opus", "flac"];

function isAudioPath(path: string): boolean {
  const dot = path.lastIndexOf(".");
  if (dot < 0) return false;
  return AUDIO_EXTENSIONS.includes(path.slice(dot + 1).toLowerCase());
}

export function AiPage({ config, apiKeys, onConfigChanged, onNavigate }: Props) {
  const baseAi = useMemo(() => mergeAi(config, {}), [config]);
  const profiles = useMemo(() => profilesForAi(baseAi), [baseAi]);
  // With no saved profiles (fresh install) fall back to a working profile
  // derived from the flat active config, purely to keep the editors below
  // bound to something. It is NOT offered in the profile picker, so a clean
  // install shows the empty state rather than a phantom OpenAI profile.
  const fallbackProfile = useMemo(() => normalizeProfile(baseAi, {}), [baseAi]);
  const activeProfile = profiles.find((item) => item.id === baseAi.active_profile_id) ?? profiles[0] ?? fallbackProfile;
  const ai = useMemo(() => activeConfigFromProfile(baseAi, activeProfile, profiles), [baseAi, activeProfile, profiles]);
  const provider = PROVIDERS.find((item) => item.id === ai.provider) ?? PROVIDERS[0];
  // A mode with an LLM that cannot be reached is a silent mode: Rust sets a
  // skipped_reason and inserts the local text, while the user simply sees that
  // "hybrid does not work". We compute it here so it can be said on the screen
  // where the mode is chosen.
  const routeBlocker = llmRouteBlocker(config, apiKeys);

  // The active profile's key, for the card at the top of the page. The manual
  // processing panel below judges by its own profile, not by this one.
  const activeKeyRef = profileKeyRef(activeProfile);
  const activeKeyInfo = apiKeys[activeKeyRef] ?? EMPTY_KEY_INFO;

  // Manual text processing may run on a different profile from dictation: they
  // have different jobs, and a model good at cleaning speech need not be the
  // best for arbitrary text. By default the voice profile is inherited.
  const textProfile = useMemo(
    () => textProfileFor(baseAi, profiles, activeProfile),
    [baseAi, profiles, activeProfile],
  );
  const textKeyRef = profileKeyRef(textProfile);
  const textKeyInfo = apiKeys[textKeyRef] ?? (textProfile.id === "default" ? apiKeys[textProfile.provider] ?? EMPTY_KEY_INFO : EMPTY_KEY_INFO);
  const textInheritsVoice = textProfile.id === activeProfile.id;

  // The built-in prompt for the profile's preset and what will actually go to
  // the model.
  const builtinPrompt = presetPrompt(activeProfile.prompt_preset);
  const promptCustom = promptIsCustom(activeProfile);
  const [promptDraft, setPromptDraft] = useState(effectiveSystemPrompt(activeProfile));
  // The tail Rust appends to any prompt, including a hand-written one. We read
  // it from the backend rather than keep a copy here: a copy would diverge from
  // the original on the very first edit, and then "what goes to the model" would
  // lie in exactly the place it is shown for.
  const [outputContract, setOutputContract] = useState("");
  const [contractShown, setContractShown] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [testLoading, setTestLoading] = useState(false);
  const [testResult, setTestResult] = useState<AiRunResult | null>(null);
  const [manualText, setManualText] = useState("");
  const [manualLoading, setManualLoading] = useState(false);
  const [manualResult, setManualResult] = useState<AiRunResult | null>(null);
  const [fileStage, setFileStage] = useState<FileStage>(null);
  const [fileResult, setFileResult] = useState<TranscribeFileResult | null>(null);
  const [fileError, setFileError] = useState("");
  // It arrives as an event at the start of a session rather than in the result:
  // by the time there is a result there is nothing left to cancel.
  const fileSessionId = useRef<number | null>(null);
  const [fileDragActive, setFileDragActive] = useState(false);
  // The drop subscription is installed once, so busyness is read from refs: a
  // closure over state would freeze the values of the first render.
  const fileStageRef = useRef<FileStage>(null);
  const manualLoadingRef = useRef(false);
  const skipResetRef = useRef(false);

  useEffect(() => {
    if (skipResetRef.current) { skipResetRef.current = false; return; }
    setPromptDraft(effectiveSystemPrompt(activeProfile));
  }, [activeProfile.id, activeProfile.system_prompt, activeProfile.prompt_preset]);

  useEffect(() => {
    // It does not block the page: until it arrives the block simply is not
    // shown.
    invoke<string>("get_output_contract").then(setOutputContract).catch(() => {});
  }, []);

  // Dragging a file into the window.
  //
  // HTML5 drop events do not reach here: the webview has Tauri's native handler
  // enabled and it intercepts them. In exchange it gives what DataTransfer
  // cannot — a path on disk, and `transcribe_audio_file` takes exactly a path.
  //
  // The event arrives for the whole window with no element binding, so the zone
  // does not track the cursor but simply highlights for the duration of the
  // drag: there is nowhere else to drop a file, and demanding precise aim would
  // be pedantry.
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | null = null;
    let disposed = false;
    void getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      if (payload.type === "enter" || payload.type === "over") { setFileDragActive(true); return; }
      setFileDragActive(false);
      if (payload.type !== "drop") return;
      const path = payload.paths.find(isAudioPath);
      if (!path) {
        if (payload.paths.length > 0) {
          setFileResult(null);
          setFileError(t("Это не аудиофайл. Поддерживаются {p0}.", { p0: AUDIO_EXTENSIONS.join(", ") }));
        }
        return;
      }
      // One transcription at a time: the engine is busy anyway, and a second
      // would queue behind the first with no trace in the interface.
      if (fileStageRef.current !== null || manualLoadingRef.current) return;
      void transcribeFile(path);
    }).then((fn) => {
      if (disposed) { fn(); return; }
      unlisten = fn;
    });
    return () => { disposed = true; unlisten?.(); };
  }, []);

  useEffect(() => { fileStageRef.current = fileStage; }, [fileStage]);
  useEffect(() => { manualLoadingRef.current = manualLoading; }, [manualLoading]);

  function showMessage(text: string) {
    setMessage(text);
    window.setTimeout(() => setMessage((current) => (current === text ? null : current)), 3500);
  }

  async function saveAi(patch: Partial<AiConfig>) {
    if (patch.active_profile_id) {
      const nextActive = profiles.find((profile) => profile.id === patch.active_profile_id) ?? activeProfile;
      await onConfigChanged({ ai_processing: activeConfigFromProfile(baseAi, nextActive, profiles) });
      return;
    }
    const updatedProfile = normalizeProfile(ai, {
      ...activeProfile,
      model: patch.model ?? activeProfile.model,
      base_url: patch.base_url ?? activeProfile.base_url,
      api_key_ref: patch.api_key_ref ?? activeProfile.api_key_ref ?? activeKeyRef,
      prompt_preset: patch.prompt_preset ?? activeProfile.prompt_preset,
      system_prompt: patch.system_prompt ?? activeProfile.system_prompt,
      llm_min_duration_seconds: patch.llm_min_duration_seconds ?? activeProfile.llm_min_duration_seconds,
      llm_timeout_seconds: patch.llm_timeout_seconds ?? activeProfile.llm_timeout_seconds,
    });
    const nextProfiles = profiles.map((profile) => profile.id === updatedProfile.id ? updatedProfile : profile);
    const nextPatch = patch.model
      ? { ...patch, provider_models: { ...(ai.provider_models ?? {}), [ai.provider]: patch.model }, profiles: nextProfiles }
      : { ...patch, profiles: nextProfiles };
    const next = mergeAi(activeConfigFromProfile(ai, updatedProfile, nextProfiles), nextPatch);
    skipResetRef.current = true;
    await onConfigChanged({ ai_processing: next });
  }

  async function savePrompt() {
    const next = promptDraft.trim();
    if (!next || next === (activeProfile.system_prompt ?? "")) return;
    skipResetRef.current = true;
    // It matched the built-in one — we save emptiness rather than a copy: a copy
    // freezes and stops receiving edits to the built-in prompt.
    await saveAi({ system_prompt: next === builtinPrompt.trim() ? "" : next });
    showMessage(t("Системный промпт сохранён."));
  }

  // Reset used to only put the text into the field and save nothing: until you
  // also pressed «Сохранить промпт», the old one came back on your next visit.
  // Hence "you have to click every time".
  async function resetPrompt() {
    setPromptDraft(builtinPrompt);
    if (!activeProfile.system_prompt?.trim()) return;
    skipResetRef.current = true;
    await saveAi({ system_prompt: "" });
    showMessage(t("Профиль снова использует встроенный промпт."));
  }

  async function runTestPrompt() {
    setTestLoading(true);
    setTestResult(null);
    try {
      const result = await invoke<AiRunResult>("test_ai_prompt", {
        profile_id: activeProfile.id,
        profile_name: activeProfile.name,
        api_key_ref: activeKeyRef,
        provider: ai.provider,
        model: ai.model,
        base_url: ai.base_url ?? "",
        system_prompt: ai.system_prompt,
        // i18n-ignore: a Russian dictation sample for the trial LLM request
        text: "ну в общем нужно сегодня встретиться с командой и обсудить следующие шаги",
      });
      setTestResult(result);
    } catch (e) {
      setTestResult({ available: false, message: e instanceof Error ? e.message : String(e) });
    } finally {
      setTestLoading(false);
    }
  }

  async function runManualProcessing() {
    const text = manualText.trim();
    if (!text) { setManualResult({ available: false, message: t("Вставьте текст для обработки.") }); return; }
    setManualLoading(true);
    setManualResult(null);
    try {
      // The fields are taken from the profile itself rather than from the flat
      // active ones: under inheritance they are the same, but when a different
      // profile is chosen the flat fields would describe the voice one — that
      // is, they would send the request somewhere other than what is shown.
      const result = await invoke<AiRunResult>("process_text_ai", {
        text,
        profile_id: textProfile.id,
        profile_name: textProfile.name,
        api_key_ref: textKeyRef,
        provider: textProfile.provider,
        model: textProfile.model,
        base_url: textProfile.base_url ?? "",
        system_prompt: textProfile.system_prompt ?? ai.system_prompt,
      });
      setManualResult(result);
    } catch (e) {
      setManualResult({ available: false, message: e instanceof Error ? e.message : String(e) });
    } finally {
      setManualLoading(false);
    }
  }

  async function runFileTranscription() {
    setFileError("");
    setFileResult(null);
    let path: string | null = null;
    try {
      path = await invoke<string | null>("pick_audio_file");
    } catch (e) {
      setFileError(e instanceof Error ? e.message : String(e));
      return;
    }
    // The user closed the dialog — that is not an error and there is nothing to
    // show.
    if (!path) return;
    await transcribeFile(path);
  }

  async function transcribeFile(path: string) {
    setFileError("");
    setFileResult(null);
    setFileStage("decoding");
    // The event arrives after decoding, but the subscription is installed before
    // the call: otherwise there is a race between `emit` in Rust and `listen`
    // here.
    const unlisten = await on<{ session_id: number }>("file-transcription-started", (payload) => {
      fileSessionId.current = payload.session_id;
      setFileStage("transcribing");
    });
    try {
      const result = await invoke<TranscribeFileResult>("transcribe_audio_file", { path });
      setFileResult(result);
    } catch (e) {
      setFileError(e instanceof Error ? e.message : String(e));
    } finally {
      unlisten();
      fileSessionId.current = null;
      setFileStage(null);
    }
  }

  async function cancelFileTranscription() {
    const sessionId = fileSessionId.current;
    if (sessionId === null) return;
    try {
      await invoke("cancel_audio_file", { session_id: sessionId });
    } catch (e) {
      setFileError(e instanceof Error ? e.message : String(e));
    }
  }

  async function copyFileResult() {
    const text = fileResult?.text ?? "";
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      showMessage(t("Результат скопирован."));
    } catch {
      showMessage(t("Не удалось скопировать."));
    }
  }

  async function copyManualResult() {
    const text = manualResult?.output ?? "";
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      showMessage(t("Результат скопирован."));
    } catch {
      showMessage(t("Не удалось скопировать."));
    }
  }

  return (
    <div className="page">
      <PageHeader
        title={t("LLM-обработка")}
        actions={<>
          {profiles.length > 0
            ? <span className="pill accent" title={t("Активный профиль · {p0} / {p1}", { p0: provider.name, p1: ai.model })}>{activeProfile.name}</span>
            : <span className="pill" title={t("Профили ещё не созданы")}>{t("нет профилей")}</span>}
          <button className="btn btn--ghost" onClick={() => onNavigate("integrations")}><Icon name="server" size={12}/>{t("Интеграции")}</button>
        </>}
      />

      {message && <div style={{ padding: "10px 12px", borderRadius: 8, background: "var(--bg-2)", border: "1px solid var(--line)", font: "500 12px/1.4 var(--font-sans)", marginBottom: 14 }}>{message}</div>}

      <section className="card" style={{ padding: 18, marginBottom: 14 }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, marginBottom: 12 }}>
          <h2 style={{ margin: 0, font: "600 14px/1.2 var(--font-sans)", display: "inline-flex", alignItems: "center", gap: 5 }}>
             {t("Активный профиль")} <Hint text={t("Один профиль = одна связка provider + ключ + модель. Создаются и редактируются они в «Интеграциях»; здесь профиль только выбирают.")}/>
          </h2>
          <button className="btn btn--ghost" onClick={() => onNavigate("integrations")}>
            <Icon name="server" size={12}/>{t("Управлять профилями")}<Icon name="arrow-right" size={11}/>
          </button>
        </div>

        {/* A picker rather than a list of rows. The full list with its own
            «Сделать активным» buttons stood here as well as in «Интеграциях» —
            two places to do the same thing, and the row actions there and here
            looked nothing alike. Switching the active profile is a frequent
            action and stays; everything else about a profile is edited where
            profiles live. */}
        {profiles.length === 0 ? (
          <div className="list-empty">
            <span>{t("Профилей пока нет. Настройки LLM ниже применяются к базовой конфигурации; профиль нужен, чтобы хранить несколько связок «провайдер + ключ + модель».")}</span>
            <button className="btn btn--ghost" onClick={() => onNavigate("integrations")}>
              <Icon name="plus" size={12}/>  {t("Создать профиль")} </button>
          </div>
        ) : (
          <div className="active-profile">
            <div className="active-profile__pick">
              <CustomSelect<string>
                value={activeProfile.id}
                inlineMeta
                options={profiles.map((profile) => {
                  const itemProvider = PROVIDERS.find((item) => item.id === profile.provider) ?? PROVIDERS[0];
                  return { value: profile.id, label: profile.name, meta: `${itemProvider.name} · ${profile.model}` };
                })}
                onChange={(next) => {
                  if (next === activeProfile.id) return;
                  void saveAi({ active_profile_id: next });
                  const picked = profiles.find((profile) => profile.id === next);
                  if (picked) showMessage(t("Активный профиль: «{p0}».", { p0: picked.name }));
                }}
              />
            </div>
            <span className={activeKeyInfo.available ? "pill ok dot mono" : "pill warn"}>
              {activeKeyInfo.available ? (activeKeyInfo.masked || t("ключ сохранён")) : t("нет ключа")}
            </span>
            <Hint text={t("Прогнать образец через активный профиль с его промптом и увидеть ответ модели (списывает токены)")}>
              <button
                className="btn btn--primary"
                disabled={testLoading || !ai.model.trim()}
                onClick={() => void runTestPrompt()}
              ><Icon name="spark" size={12}/>{testLoading ? t("Отправляю…") : t("Пробный запрос")}</button>
            </Hint>
          </div>
        )}
          {testResult && (
            <div style={{ display: "grid", gap: 6, marginTop: 12, padding: 10, borderRadius: 8, background: "var(--bg-2)", border: "1px solid var(--line)" }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span className={testResult.available && !testResult.fallback ? "pill ok" : "pill warn"}>{testResult.available ? t("Ответ получен") : (testResult.fallback ? t("Запрос отправлен, fallback") : t("Запрос не отправлен"))}</span>
                <Hint text={t("Закрыть")} style={{ marginLeft: "auto" }}>
                  <button className="icon-btn" aria-label={t("Закрыть")} onClick={() => setTestResult(null)}><Icon name="x" size={12}/></button>
                </Hint>
              </div>
              {(testResult.message || testResult.provider_error || testResult.skipped_reason) && <div style={{ font: "500 11px/1.45 var(--font-mono)", color: testResult.available ? "var(--text-mute)" : "var(--err)", whiteSpace: "pre-wrap" }}>{testResult.provider_error || testResult.message || testResult.skipped_reason}</div>}
              <ProviderSnippet result={testResult}/>
              {testResult.output && <div style={{ padding: 10, borderRadius: 6, background: "var(--surface-2)", border: "1px solid var(--border)", font: "400 12px/1.5 var(--font-sans)", whiteSpace: "pre-wrap" }}>{testResult.output}</div>}
            </div>
          )}
        </section>

        <section className="card" style={{ padding: 18, marginBottom: 14 }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, marginBottom: 12 }}>
            <h2 style={{ margin: 0, font: "600 14px/1.2 var(--font-sans)" }}>{t("Режим обработки")}</h2>
            <span style={{ font: "400 11.5px/1 var(--font-sans)", color: "var(--ink-mute)" }}>{t("текущий:")} <span className="mono" style={{ color: "var(--accent-text)" }}>{ai.pipeline_mode}</span></span>
          </div>
          <div className="ai-mode-grid">
            {PIPELINE_MODES().map((mode) => (
              <button key={mode.id} className="ai-mode-card" data-selected={ai.pipeline_mode === mode.id} onClick={() => void saveAi({ pipeline_mode: mode.id })} type="button">
                <div className="ai-mode-card__head">
                  <span className="ai-mode-card__icon"><Icon name={mode.icon} size={15}/></span>
                  <span className="ai-mode-card__title">{mode.title}</span>
                </div>
                <div className="ai-mode-card__desc">{mode.sub}</div>
              </button>
            ))}
          </div>
          {/* One line with no heading and no button: the route to «Интеграции»
              already lies two cards above, and a heading would repeat what the
              reason itself says. */}
          {routeBlocker && (
            <div role="alert" className="ai-mode-warning">
              <Icon name="info" size={13} style={{ color: "var(--warn)", flex: "0 0 auto", marginTop: 1 }}/>
              <span style={{ font: "500 11.5px/1.45 var(--font-sans)", color: "var(--ink)" }}>
                {blockerReason(routeBlocker)}{" "}
                {ai.pipeline_mode === "cloud"
                  ? t("Пока этого нет, распознавать нечем: диктовка завершится ошибкой.")
                  : t("Пока этого нет, диктовка вставляет локальный текст без обработки LLM.")}
              </span>
            </div>
          )}
        </section>

        <section className="card" style={{ padding: "4px 22px", marginBottom: 14 }}>
          <SettingRow title={t("Системный промпт")} stack hint={t("Инструкции, которые отправляются модели перед каждым запросом. Шаблон поддерживает плейсхолдеры {{language}} и {{transcript}}.")}>
            <div style={{ display: "grid", gap: 8 }}>
              {/* A profile with its own text silently stops receiving edits to
                  the built-in prompt. While that was invisible, a config from
                  April went around without the rule "do not replace words with
                  synonyms". */}
              {promptCustom && (
                <div className="flex-row" style={{ gap: 8, flexWrap: "wrap", padding: "7px 10px", borderRadius: "var(--r-sm)", background: "var(--warn-soft)", border: "1px solid rgba(251,191,36,0.30)" }}>
                  <Icon name="info" size={13} style={{ color: "var(--warn)", flex: "0 0 auto" }}/>
                  <span style={{ font: "500 11.5px/1.4 var(--font-sans)", color: "var(--ink)" }}>
                    {t("У профиля свой промпт — правки встроенного до него не доходят.")}
                  </span>
                  <button className="btn btn--ghost" type="button" onClick={() => void resetPrompt()} style={{ height: 24, padding: "0 8px", fontSize: 11, marginLeft: "auto" }}>
                    <Icon name="refresh" size={11}/>{t("Вернуть встроенный")}
                  </button>
                </div>
              )}
              <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                <span style={{ font: "600 10px/1 var(--font-mono)", color: "var(--ink-mute)", textTransform: "uppercase", letterSpacing: "0.05em" }}>{t("Пресеты:")}</span>
                {SYSTEM_PROMPT_PRESETS().map((preset) => {
                  const active = promptDraft.trim() === preset.prompt.trim();
                  return (
                    <Hint key={preset.id} text={preset.description}>
                      <button
                        type="button"
                        className={active ? "btn btn--primary" : "btn btn--ghost"}
                        onClick={() => setPromptDraft(preset.prompt)}
                        style={{ height: 26 }}
                      >
                        {preset.label}
                      </button>
                    </Hint>
                  );
                })}
                <span style={{ font: "400 11px/1.3 var(--font-sans)", color: "var(--ink-mute)" }}>
                   {t("Кликни пресет, сохрани и протестируй на длинной записи через «Повторить LLM» в истории.")} </span>
              </div>
              {/* There is deliberately no "expand" button: the box is a dozen
                  lines tall as it is, a visible scrollbar shows how much text is
                  left, and the height can always be dragged by the corner. */}
              <textarea
                className="field mono scroll-visible"
                value={promptDraft}
                onChange={(e) => setPromptDraft(e.target.value)}
                rows={12}
                style={{ width: "100%", resize: "vertical", fontSize: 12 }}
                spellCheck={false}
              />
              <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                <button
                  className="btn btn--primary"
                  onClick={() => void savePrompt()}
                  disabled={!promptDraft.trim() || promptDraft === (activeProfile.system_prompt ?? "")}
                >
                  <Icon name="check" size={12}/>{t("Сохранить промпт")} </button>
                <Hint text={t("Вернуть встроенный промпт и снова получать его правки")}>
                  <button
                    className="btn btn--ghost"
                    onClick={() => void resetPrompt()}
                    disabled={!promptCustom && promptDraft.trim() === builtinPrompt.trim()}
                  >
                    <Icon name="refresh" size={12}/>{t("Вернуть встроенный")} </button>
                </Hint>
                <span style={{ marginLeft: "auto", font: "500 11px/1 var(--font-mono)", color: "var(--ink-mute)" }}>
                  {promptDraft.length}{outputContract ? ` + ${outputContract.length}` : ""}  {t("симв.")}</span>
              </div>

              {outputContract ? (
                <div style={{ borderTop: "1px solid var(--border)", paddingTop: 10, display: "grid", gap: 8 }}>
                  <button
                    className="btn btn--ghost"
                    type="button"
                    onClick={() => setContractShown((current) => !current)}
                    style={{ justifySelf: "start" }}
                  >
                    <Icon name={contractShown ? "chev-up" : "chev-down"} size={12}/>
                    {t("Что приложение дописывает к промпту")} </button>
                  {contractShown ? (
                    <>
                      <p style={{ margin: 0, font: "400 11.5px/1.45 var(--font-sans)", color: "var(--ink-mute)" }}>
                         {t("Эти правила отправляются после твоего промпта при каждом запросе — они одинаковы для всех пресетов и профилей, и их нельзя отредактировать. Показаны, чтобы было видно, что модель получает целиком.")} </p>
                      <textarea
                        className="field mono"
                        value={outputContract}
                        readOnly
                        rows={10}
                        style={{ width: "100%", resize: "vertical", fontSize: 12, opacity: 0.75, cursor: "default" }}
                        spellCheck={false}
                        aria-label={t("Что приложение дописывает к промпту")}
                      />
                    </>
                  ) : null}
                </div>
              ) : null}
            </div>
          </SettingRow>

          <div className="split-setting-grid">
            <div className="split-setting-cell">
              <div>
                <h3>{t("Порог LLM")}<Hint text={t("LLM запускается только для записей не короче этого значения. 0 = обрабатывать все.")}/></h3>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <NumberField className="mono" min={0} step={5} value={ai.llm_min_duration_seconds ?? 0}
                  onValueChange={(next) => void saveAi({ llm_min_duration_seconds: Math.max(0, Number(next) || 0) })} style={{ width: 90, height: 34 }}/>
                <span style={{ font: "500 12px/1 var(--font-sans)", color: "var(--text-2)" }}>{t("секунд")}</span>
              </div>
            </div>
            <div className="split-setting-cell">
              <div>
                <h3>{t("Таймаут LLM")}<Hint text={t("Если провайдер не ответит за это время — вставится локально обработанный текст и fallback запишется в историю.")}/></h3>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <NumberField className="mono" min={1} max={60} step={1} value={ai.llm_timeout_seconds ?? 12}
                  onValueChange={(next) => void saveAi({ llm_timeout_seconds: Math.max(1, Math.min(60, Number(next) || 12)) })} style={{ width: 90, height: 34 }}/>
                <span style={{ font: "500 12px/1 var(--font-sans)", color: "var(--text-2)" }}>{t("секунд")}</span>
              </div>
            </div>
          </div>
        </section>

        <section className="card" style={{ padding: 18 }}>
          <div style={{ display: "flex", justifyContent: "space-between", marginBottom: 10, gap: 10, flexWrap: "wrap" }}>
            <div>
              <div style={{ font: "600 14px/1.2 var(--font-sans)", color: "var(--ink)" }}>{t("Обработать текст")}</div>
              <div style={{ font: "400 11.5px/1.3 var(--font-sans)", color: "var(--ink-mute)", marginTop: 2 }}>{t("Любой текст, не связанный с диктовкой")}</div>
            </div>
            {/* This used to say only "by the active profile's rules" — with no
                way to learn which model. The selector both names it and lets it
                be separated from dictation. */}
            <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 230 }}>
              <span className="set-label" style={{ whiteSpace: "nowrap" }}>{t("Обрабатывает")}</span>
              <CustomSelect
                value={textInheritsVoice ? "" : textProfile.id}
                options={[
                  { value: "", label: t("Как для диктовки"), meta: `${activeProfile.name} · ${activeProfile.model}`, icon: "mic" },
                  ...profiles
                    .filter((profile) => profile.id !== activeProfile.id)
                    .map((profile) => ({
                      value: profile.id,
                      label: profile.name,
                      meta: `${PROVIDERS.find((item) => item.id === profile.provider)?.name ?? profile.provider} · ${profile.model}`,
                      icon: "spark",
                    })),
                ]}
                onChange={(next) => void onConfigChanged({ ai_processing: mergeAi(baseAi, { text_profile_id: next }) })}
                className="custom-select--grow"
              />
            </div>
          </div>
          <textarea className="field mono" value={manualText} onChange={(e) => setManualText(e.target.value)} placeholder={t("Вставьте текст для обработки через выбранную LLM")} style={{ width: "100%", minHeight: 150, padding: 12, resize: "vertical", lineHeight: 1.55 }}/>
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 10, flexWrap: "wrap" }}>
            <button className="btn btn--primary" onClick={() => void runManualProcessing()} disabled={manualLoading || !manualText.trim() || !textProfile.model.trim()}>
              <Icon name="spark" size={12}/>{manualLoading ? t("Обрабатываю…") : t("Обработать")}
            </button>
            {!textProfile.model.trim() && <span style={{ font: "500 11px/1.35 var(--font-mono)", color: "var(--err)" }}>{t("model не задан")}</span>}
            {!textKeyInfo.available && <span style={{ font: "500 11px/1.35 var(--font-mono)", color: "var(--err)" }}>{t("Ключ профиля не задан.")}</span>}
          </div>
          {/* Its own zone rather than another button in the row: higher up the
              card is text entry, here is speech, and these are two different
              entrances into processing. They used to be separated only by a
              second row of buttons. */}
          <div className="audio-drop" data-active={fileDragActive ? "true" : "false"} data-busy={fileStage !== null ? "true" : "false"}>
            <span className="audio-drop__mark"><Icon name="mic" size={16}/></span>
            <div className="audio-drop__copy">
              <strong>
                {fileStage === null ? t("Расшифровать аудиофайл") : (fileStage === "decoding" ? t("Читаю файл…") : t("Распознаю аудио…"))}
                {fileStage === null && <span className="audio-drop__note">{t("поддерживается drag-and-drop")}</span>}
              </strong>
              <span className="audio-drop__formats">{fileStage === null ? AUDIO_EXTENSIONS.join(", ") : t("Файл распознаётся локальной моделью")}</span>
            </div>
            <div className="audio-drop__actions">
              {fileStage === "transcribing"
                /* Cancellation exists only during the recognition stage: before
                   it there is no session yet, and decoding is short by
                   definition. */
                ? <button className="btn btn--ghost" onClick={() => void cancelFileTranscription()}>{t("Отменить")}</button>
                : (
                  <button className="btn btn--ghost" onClick={() => void runFileTranscription()} disabled={fileStage !== null || manualLoading}>
                    <Icon name="folder" size={12}/>{t("Выбрать файл")}
                  </button>
                )}
            </div>
          </div>
          {fileError && (
            <div style={{ marginTop: 10, font: "500 11px/1.45 var(--font-mono)", color: "var(--err)", whiteSpace: "pre-wrap" }}>{fileError}</div>
          )}
          {fileResult && (
            <div style={{ display: "grid", gap: 8, marginTop: 12 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <FileStatusPill result={fileResult}/>
                <span style={{ font: "500 11px/1.35 var(--font-mono)", color: "var(--ink-mute)" }}>
                  {t("{p0} с аудио", { p0: Math.round(fileResult.audio_seconds) })}
                </span>
                {fileResult.text && <button className="btn btn--ghost" onClick={() => void copyFileResult()}><Icon name="copy" size={12}/>{t("Скопировать")}</button>}
              </div>
              {fileResult.ai_status?.skipped_reason && (
                <div style={{ font: "500 11px/1.45 var(--font-mono)", color: "var(--ink-mute)", whiteSpace: "pre-wrap" }}>{fileResult.ai_status.skipped_reason}</div>
              )}
              <div style={{ padding: 12, borderRadius: "var(--r-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 13px/1.55 var(--font-sans)", color: "var(--ink)", whiteSpace: "pre-wrap" }}>{fileResult.text}</div>
              {/* We show the raw stage only when it differs — otherwise it is
                  simply a second copy of the same text. */}
              {fileResult.raw_text !== fileResult.text && (
                <details>
                  <summary style={{ cursor: "pointer", font: "500 11px/1.4 var(--font-sans)", color: "var(--ink-mute)" }}>{t("Whisper без обработки")}</summary>
                  <div style={{ marginTop: 6, padding: 12, borderRadius: "var(--r-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 13px/1.55 var(--font-sans)", color: "var(--ink-mute)", whiteSpace: "pre-wrap" }}>{fileResult.raw_text}</div>
                </details>
              )}
            </div>
          )}
          {manualResult && (
            <div style={{ display: "grid", gap: 8, marginTop: 12 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span className={manualResult.available && !manualResult.fallback ? "pill ok" : (manualResult.fallback ? "pill warn" : "pill err")}>{manualResult.available ? (manualResult.fallback ? "Fallback" : t("Готово")) : t("Не обработано")}</span>
                {manualResult.output && <button className="btn btn--ghost" onClick={() => void copyManualResult()}><Icon name="copy" size={12}/>{t("Скопировать")}</button>}
              </div>
              {(manualResult.message || manualResult.provider_error || manualResult.skipped_reason) && <div style={{ font: "500 11px/1.45 var(--font-mono)", color: manualResult.provider_error || manualResult.skipped_reason ? "var(--err)" : "var(--ink-mute)", whiteSpace: "pre-wrap" }}>{manualResult.provider_error || manualResult.message || manualResult.skipped_reason}</div>}
              <ProviderSnippet result={manualResult}/>
              {manualResult.output && <div style={{ padding: 12, borderRadius: "var(--r-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 13px/1.55 var(--font-sans)", color: "var(--ink)", whiteSpace: "pre-wrap" }}>{manualResult.output}</div>}
            </div>
          )}
        </section>
    </div>
  );
}
