import { useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { invoke, on } from "../bridge";
import { Card, CardHead, PageHeader, Segmented } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Hint } from "../components/Hint";
import {
  activeConfigFromProfile,
  effectiveSystemPrompt,
  gapReason,
  mergeAi,
  presetPrompt,
  promptIsCustom,
  normalizeProfile,
  profileGap,
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
import { confirmAction } from "../components/ConfirmDialog";
import { isLocalBaseUrl } from "./baseUrlFormat";
import { NumberField } from "../components/NumberField";
import type { ApiKeyStatus, ConfigResult } from "../bridge/types";
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
      <pre className="scroll-visible" style={{ margin: "6px 0 0", padding: 10, maxHeight: 180, overflow: "auto", borderRadius: "var(--radius-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 11px/1.5 var(--font-mono)", color: "var(--ink-mute)", whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
        {result.response_snippet}
      </pre>
    </details>
  );
}

const PIPELINE_MODES = () => ([
  { id: "local", title: t("Только локально"), sub: t("STT на этом компьютере. LLM не вызывается."), icon: "shield" },
  { id: "hybrid", title: t("Локальное распознавание + LLM"), sub: t("Распознаем локально, затем отправляем текст в LLM для обработки."), icon: "wand" },
  { id: "cloud", title: t("Облачное распознавание"), sub: t("Запись целиком уходит провайдеру и распознаётся у него. Локальная модель не нужна."), icon: "spark" },
] as const);

/** Why a request cannot leave. One wording and one shape for the dictation
 *  route and for the manual panel below, which may run on another profile: the
 *  two used to describe the same two states in different words and different
 *  colours. */
function GapNote({ gap, consequence }: { gap: LlmRouteBlocker; consequence?: string }) {
  if (!gap) return null;
  return (
    <div role="alert" className="route-note">
      <Icon name="info" size={13} style={{ color: "var(--warn)", flex: "0 0 auto", marginTop: 1 }}/>
      <span style={{ font: "500 11.5px/1.45 var(--font-sans)", color: "var(--ink)" }}>
        {gapReason(gap)}{consequence ? ` ${consequence}` : ""}
      </span>
    </div>
  );
}

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
  // No key pill next to the picker: which key a profile is bound to is decided
  // in «Интеграциях», and the one thing this page needs to know about it —
  // that it is missing — the route note below says in words.
  const activeKeyRef = profileKeyRef(activeProfile);

  // Manual text processing may run on a different profile from dictation: they
  // have different jobs, and a model good at cleaning speech need not be the
  // best for arbitrary text. By default the voice profile is inherited.
  const textProfile = useMemo(
    () => textProfileFor(baseAi, profiles, activeProfile),
    [baseAi, profiles, activeProfile],
  );
  const textKeyRef = profileKeyRef(textProfile);
  const textGap = profileGap(textProfile, apiKeys);
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
  const [advancedShown, setAdvancedShown] = useState(false);
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

  /** A trial request is a real request: it goes to the provider and it spends
   *  tokens, off one click and with nothing of the user's own in it. The hint
   *  on the button said so, and a hint is read after the click at best.
   *
   *  Not asked for a provider on this machine — a local endpoint costs nothing,
   *  and a modal in front of a free request is friction that teaches people to
   *  dismiss the modal. */
  async function runTestPrompt() {
    if (!isLocalBaseUrl(ai.base_url)) {
      const confirmed = await confirmAction(
        t("Отправить пробный запрос в {p0} ({p1})? Запрос уйдёт провайдеру и спишет токены.", { p0: provider.name, p1: ai.model }),
        { label: t("Отправить"), icon: "spark" },
      );
      if (!confirmed) return;
    }
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
        // Resolved against the preset rather than passed on raw: a profile that
        // never edited its prompt stores an empty string, and `??` lets an
        // empty string through — so the panel ran the chosen profile on the
        // dictation profile's prompt, and a «structured» preset quietly asked
        // for plain paragraphs.
        system_prompt: effectiveSystemPrompt(textProfile),
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
      {/* No actions in the header. The profile pill repeated the picker two
          rows below it, and «Интеграции» repeated «Управлять профилями» in the
          same row as that picker — a second copy of both, further from what
          they act on. */}
      <PageHeader title={t("LLM-обработка")}/>

      {message && <div style={{ padding: "10px 12px", borderRadius: 8, background: "var(--bg-2)", border: "1px solid var(--line)", font: "500 12px/1.4 var(--font-sans)", marginBottom: 14 }}>{message}</div>}

      <div className="card-stack">
        {/* Mode and profile are one chain, and they used to be two cards: the
            mode decides whether an LLM is called at all, the profile supplies
            the provider, key and model it is called with, and the blocker says
            where the chain is broken. Reading «why is hybrid silent?» meant
            tying the key pill in one card to the warning in the next. */}
        <Card>
          <CardHead
            title={t("Маршрут обработки")}
            hint={t("Что происходит с записью после диктовки: где распознаётся речь и вызывается ли LLM.")}
            actions={
              <span style={{ font: "400 11.5px/1 var(--font-sans)", color: "var(--ink-mute)" }}>{t("текущий:")} <span className="mono" style={{ color: "var(--accent-text)" }}>{ai.pipeline_mode}</span></span>
            }
          />
          {/* A radiogroup, not three loose buttons: it is a choice of one of
              three, and a screen reader announced it as three unrelated
              controls with no arrow-key movement between them. */}
          <div className="ai-mode-grid" role="radiogroup" aria-label={t("Режим обработки")}>
            {PIPELINE_MODES().map((mode, index, modes) => {
              const selected = ai.pipeline_mode === mode.id;
              return (
                <button
                  key={mode.id}
                  type="button"
                  className="ai-mode-card"
                  role="radio"
                  aria-checked={selected}
                  tabIndex={selected ? 0 : -1}
                  data-selected={selected}
                  onClick={() => void saveAi({ pipeline_mode: mode.id })}
                  onKeyDown={(e) => {
                    const step = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1
                      : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
                    if (!step) return;
                    e.preventDefault();
                    const next = modes[(index + step + modes.length) % modes.length];
                    void saveAi({ pipeline_mode: next.id });
                    const grid = e.currentTarget.parentElement;
                    grid?.querySelectorAll<HTMLButtonElement>(".ai-mode-card")[modes.indexOf(next)]?.focus();
                  }}
                >
                  <div className="ai-mode-card__head">
                    <span className="ai-mode-card__icon"><Icon name={mode.icon} size={15}/></span>
                    <span className="ai-mode-card__title">{mode.title}</span>
                  </div>
                  <div className="ai-mode-card__desc">{mode.sub}</div>
                </button>
              );
            })}
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
                <Icon name="plus" size={12}/>{t("Создать профиль")}</button>
            </div>
          ) : (
            <div className="active-profile">
              <span className="picker-label">{t("Профиль")}</span>
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
              <Hint text={t("Прогнать образец через активный профиль с его промптом и увидеть ответ модели (списывает токены)")}>
                <button
                  className="btn btn--primary"
                  disabled={testLoading || !ai.model.trim()}
                  onClick={() => void runTestPrompt()}
                ><Icon name="spark" size={12}/>{testLoading ? t("Отправляю…") : t("Пробный запрос")}</button>
              </Hint>
              <button className="btn btn--ghost" onClick={() => onNavigate("integrations")}>
                <Icon name="server" size={12}/>{t("Управлять профилями")}
              </button>
              <button
                className="btn btn--ghost"
                type="button"
                aria-expanded={advancedShown}
                onClick={() => setAdvancedShown((current) => !current)}
              >
                <Icon name={advancedShown ? "chev-up" : "chev-down"} size={12}/>{t("Дополнительно")}
              </button>
            </div>
          )}

          {/* Set once and then never touched — the same case the «Дополнительно»
              block in «Настройках» is collapsed for. They stay on this page and
              not there because the first two are stored on the profile, and
              «Настройки» has no notion of which profile is active: two fields
              that look like app settings would quietly edit whichever profile
              happened to be picked here. */}
          {advancedShown && (
            <div className="route-advanced">
              <div className="route-advanced__cell">
                <h3>{t("Порог LLM")}<Hint text={t("LLM запускается только для записей не короче этого значения. 0 = обрабатывать все. Значение своё у каждого профиля.")}/></h3>
                <NumberField className="mono" min={0} step={5} value={ai.llm_min_duration_seconds ?? 0}
                  onValueChange={(next) => void saveAi({ llm_min_duration_seconds: Math.max(0, Number(next) || 0) })} style={{ width: 84 }}/>
                <span className="route-advanced__unit">{t("секунд")}</span>
              </div>
              <div className="route-advanced__cell">
                <h3>{t("Таймаут LLM")}<Hint text={t("Если провайдер не ответит за это время — вставится локально обработанный текст и fallback запишется в историю. Значение своё у каждого профиля.")}/></h3>
                <NumberField className="mono" min={1} max={60} step={1} value={ai.llm_timeout_seconds ?? 12}
                  onValueChange={(next) => void saveAi({ llm_timeout_seconds: Math.max(1, Math.min(60, Number(next) || 12)) })} style={{ width: 84 }}/>
                <span className="route-advanced__unit">{t("секунд")}</span>
              </div>
              {/* Read by Rust since cloud transcription existed, and until now
                  changeable only by hand-editing config.json. */}
              <div className="route-advanced__cell">
                <h3>{t("Таймаут облачного STT")}<Hint text={t("Сколько ждать ответа /audio/transcriptions в облачном режиме. Распознавать больше нечем, поэтому по истечении диктовка завершится ошибкой. Значение общее для всех профилей.")}/></h3>
                <NumberField className="mono" min={5} max={300} step={5} value={ai.cloud_stt_timeout_seconds ?? 45}
                  onValueChange={(next) => void saveAi({ cloud_stt_timeout_seconds: Math.max(5, Math.min(300, Number(next) || 45)) })} style={{ width: 84 }}/>
                <span className="route-advanced__unit">{t("секунд")}</span>
              </div>
            </div>
          )}

          {/* In local-only mode nothing above the profile is used. Saying so
              beats dimming the row: the profile can still be prepared here, and
              a greyed-out control that answers clicks is its own puzzle. */}
          {ai.pipeline_mode === "local"
            ? <p className="route-idle">{t("В этом режиме профиль и промпт не используются: LLM не вызывается.")}</p>
            : <GapNote
                gap={routeBlocker}
                consequence={ai.pipeline_mode === "cloud"
                  ? t("Пока этого нет, распознавать нечем: диктовка завершится ошибкой.")
                  : t("Пока этого нет, диктовка вставляет локальный текст без обработки LLM.")}
              />}
          {testResult && (
            <div style={{ display: "grid", gap: 6, marginTop: 12, padding: 10, borderRadius: 8, background: "var(--bg-2)", border: "1px solid var(--line)" }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span className={testResult.available && !testResult.fallback ? "pill ok" : "pill warn"}>{testResult.available ? t("Ответ получен") : (testResult.fallback ? t("Запрос отправлен, fallback") : t("Запрос не отправлен"))}</span>
                <Hint text={t("Закрыть")} style={{ marginLeft: "auto" }}>
                  <button className="icon-btn" aria-label={t("Закрыть")} onClick={() => setTestResult(null)}><Icon name="x" size={12}/></button>
                </Hint>
              </div>
              {(testResult.message || testResult.provider_error || testResult.skipped_reason) && <div style={{ font: "500 11px/1.45 var(--font-mono)", color: testResult.available ? "var(--ink-mute)" : "var(--err)", whiteSpace: "pre-wrap" }}>{testResult.provider_error || testResult.message || testResult.skipped_reason}</div>}
              <ProviderSnippet result={testResult}/>
              {testResult.output && <div style={{ padding: 10, borderRadius: 6, background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 12px/1.5 var(--font-sans)", whiteSpace: "pre-wrap" }}>{testResult.output}</div>}
            </div>
          )}
        </Card>

        <Card>
          {/* The presets are a choice of one of two, so they are a Segmented in
              the card's head. As a row of buttons they shared a flex line with
              a sentence of prose, and on a narrow window the two wrapped into
              each other. */}
          <CardHead
            title={t("Системный промпт")}
            hint={t("Инструкции, которые отправляются модели перед каждым запросом. Шаблон поддерживает плейсхолдеры {{language}} и {{transcript}}. Выбери пресет, сохрани и протестируй на длинной записи через «Обработать через LLM» в истории.")}
            actions={
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <span className="picker-label">{t("Пресет")}</span>
                <Segmented
                  value={SYSTEM_PROMPT_PRESETS().find((preset) => promptDraft.trim() === preset.prompt.trim())?.id ?? ""}
                  options={SYSTEM_PROMPT_PRESETS().map((preset) => ({ value: preset.id, label: preset.label }))}
                  onChange={(next) => {
                    const preset = SYSTEM_PROMPT_PRESETS().find((item) => item.id === next);
                    if (preset) setPromptDraft(preset.prompt);
                  }}
                />
              </div>
            }
          />
          <div>
            <div style={{ display: "grid", gap: 8 }}>
              {/* A profile with its own text silently stops receiving edits to
                  the built-in prompt. While that was invisible, a config from
                  April went around without the rule "do not replace words with
                  synonyms". */}
              {promptCustom && (
                <div className="flex-row" style={{ gap: 8, flexWrap: "wrap", padding: "7px 10px", borderRadius: "var(--radius-sm)", background: "var(--warn-soft)", border: "1px solid rgba(251,191,36,0.30)" }}>
                  <Icon name="info" size={13} style={{ color: "var(--warn)", flex: "0 0 auto" }}/>
                  <span style={{ font: "500 11.5px/1.4 var(--font-sans)", color: "var(--ink)" }}>
                    {t("У профиля свой промпт — правки встроенного до него не доходят.")}
                  </span>
                  <button className="btn btn--ghost" type="button" onClick={() => void resetPrompt()} style={{ height: 24, padding: "0 8px", fontSize: 11, marginLeft: "auto" }}>
                    <Icon name="refresh" size={11}/>{t("Вернуть встроенный")}
                  </button>
                </div>
              )}
              {/* There is deliberately no "expand" button: a visible scrollbar
                  shows how much text is left, and the height can always be
                  dragged by the corner. Eight rows rather than twelve — at
                  twelve the box filled half the window and pushed the panel
                  below it off the bottom edge. */}
              <textarea
                className="field mono scroll-visible"
                value={promptDraft}
                onChange={(e) => setPromptDraft(e.target.value)}
                rows={8}
                style={{ width: "100%", resize: "vertical", fontSize: 12 }}
                spellCheck={false}
              />
              <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                <button
                  className="btn btn--primary"
                  onClick={() => void savePrompt()}
                  disabled={!promptDraft.trim() || promptDraft === (activeProfile.system_prompt ?? "")}
                >
                  <Icon name="check" size={12}/>{t("Сохранить промпт")}</button>
                <Hint text={t("Вернуть встроенный промпт и снова получать его правки")}>
                  <button
                    className="btn btn--ghost"
                    onClick={() => void resetPrompt()}
                    disabled={!promptCustom && promptDraft.trim() === builtinPrompt.trim()}
                  >
                    <Icon name="refresh" size={12}/>{t("Вернуть встроенный")}</button>
                </Hint>
                <span style={{ marginLeft: "auto", font: "500 11px/1 var(--font-mono)", color: "var(--ink-mute)" }}>
                  {promptDraft.length}{outputContract ? ` + ${outputContract.length}` : ""}  {t("симв.")}</span>
              </div>

              {outputContract ? (
                <div style={{ borderTop: "1px solid var(--line)", paddingTop: 10, display: "grid", gap: 8 }}>
                  <button
                    className="btn btn--ghost"
                    type="button"
                    onClick={() => setContractShown((current) => !current)}
                    style={{ justifySelf: "start" }}
                  >
                    <Icon name={contractShown ? "chev-up" : "chev-down"} size={12}/>
                    {t("Что приложение дописывает к промпту")}</button>
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
          </div>
        </Card>

        {/* The «Обрабатывает» selector used to say only "by the active
            profile's rules" — with no way to learn which model. It both names
            it and lets it be separated from dictation. */}
        <Card>
          <CardHead
            title={t("Обработать текст")}
            actions={profiles.length > 0 && (
            <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 230 }}>
              <span className="picker-label">{t("Обрабатывает")}</span>
              {/* The same picker as in the route card: profile name, then
                  provider and model. It used to lead with «Как для диктовки»
                  and two icons, which named the arrangement rather than the
                  thing chosen — and named it in a vocabulary this row was alone
                  in using. Inheritance did not go away, it just stopped being a
                  row of its own: picking the dictation profile writes the empty
                  id that keeps following it. */}
              <CustomSelect<string>
                value={textInheritsVoice ? activeProfile.id : textProfile.id}
                inlineMeta
                options={profiles.map((profile) => {
                  const itemProvider = PROVIDERS.find((item) => item.id === profile.provider) ?? PROVIDERS[0];
                  return { value: profile.id, label: profile.name, meta: `${itemProvider.name} · ${profile.model}` };
                })}
                onChange={(next) => void onConfigChanged({ ai_processing: mergeAi(baseAi, { text_profile_id: next === activeProfile.id ? "" : next }) })}
                className="custom-select--grow"
              />
            </div>
            )}
          />
          <textarea className="field mono" value={manualText} onChange={(e) => setManualText(e.target.value)} placeholder={t("Вставьте текст для обработки через выбранную LLM")} style={{ width: "100%", minHeight: 150, padding: 12, resize: "vertical", lineHeight: 1.55 }}/>
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 10, flexWrap: "wrap" }}>
            <button className="btn btn--primary" onClick={() => void runManualProcessing()} disabled={manualLoading || !manualText.trim() || !!textGap}>
              <Icon name="spark" size={12}/>{manualLoading ? t("Обрабатываю…") : t("Обработать")}
            </button>
          </div>
          <GapNote gap={textGap} consequence={textInheritsVoice ? undefined : t("Панель работает на профиле «{p0}».", { p0: textProfile.name })}/>
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
              <div style={{ padding: 12, borderRadius: "var(--radius-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 13px/1.55 var(--font-sans)", color: "var(--ink)", whiteSpace: "pre-wrap" }}>{fileResult.text}</div>
              {/* We show the raw stage only when it differs — otherwise it is
                  simply a second copy of the same text. */}
              {fileResult.raw_text !== fileResult.text && (
                <details>
                  <summary style={{ cursor: "pointer", font: "500 11px/1.4 var(--font-sans)", color: "var(--ink-mute)" }}>{t("Whisper без обработки")}</summary>
                  <div style={{ marginTop: 6, padding: 12, borderRadius: "var(--radius-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 13px/1.55 var(--font-sans)", color: "var(--ink-mute)", whiteSpace: "pre-wrap" }}>{fileResult.raw_text}</div>
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
              {manualResult.output && <div style={{ padding: 12, borderRadius: "var(--radius-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 13px/1.55 var(--font-sans)", color: "var(--ink)", whiteSpace: "pre-wrap" }}>{manualResult.output}</div>}
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}
