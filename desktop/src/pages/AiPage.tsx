import { useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { invoke } from "../bridge";
import { Card, CardHead, PageHeader, Segmented } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Hint } from "../components/Hint";
import {
  activeConfigFromProfile,
  activeProfileOf,
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
  routeFields,
  SYSTEM_PROMPT_PRESETS,
  textProfileFor,
  type AiConfig,
  type LlmRouteBlocker,
} from "./aiShared";
import { CustomSelect } from "../components/CustomSelect";
import { confirmAction } from "../components/ConfirmDialog";
import { isLocalBaseUrl } from "./baseUrlFormat";
import { NumberField } from "../components/NumberField";
import type { ApiKeyStatus, ConfigChange, ConfigResult, LateAnswerMode, ReasoningMode } from "../bridge/types";
import { t } from "../i18n";
import { useFileTranscription, type FileStage, type TranscribeFileResult } from "./useFileTranscription";
import { aiFallbackLabel, defaultStage, TranscriptStages, TranscriptStats, type TextStage, type TranscriptRecord } from "./transcriptStages";

/** Mirrors `MIN_CUSTOM_OUTPUT_TOKENS` in ai/model_params.rs: below it Rust treats the limit as unset. */
const MIN_OUTPUT_LIMIT = 256;
/** Where a newly chosen custom limit starts. */
const CUSTOM_OUTPUT_LIMIT_DEFAULT = 8192;

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

/** The status pill for a file result.
 *
 *  Separate from the «Обработать текст» pills: there `available === false`
 *  means "the LLM did not run", and that is an error. Here the LLM may
 *  legitimately not run — in local-only mode it should not — while the text is
 *  transcribed all the same. A red pill on a successful transcription would be
 *  a lie. A plain transcription needs no pill at all; only an LLM that failed
 *  to deliver is worth a word next to the result. */
function FileStatusPill({ result }: { result: TranscribeFileResult }) {
  const ai = result.ai_status;
  if (ai?.fallback) return <span className="pill warn">Fallback</span>;
  if (ai?.attempted && !ai.used) return <span className="pill warn">{t("LLM не отработала")}</span>;
  return null;
}

/** A file result in the shape history uses, so both show the same stages. */
function fileRecord(result: TranscribeFileResult): TranscriptRecord {
  const ai = result.ai_status ?? undefined;
  const stt = result.inference_time_ms / 1000;
  const llm = ai?.attempted ? ai.elapsed_seconds : undefined;
  return {
    text: result.text,
    raw_text: result.raw_text,
    formatted_text: result.formatted_text,
    ai_processing: ai,
    processing_stats: { audio_seconds: result.audio_seconds, whisper_seconds: stt, llm_seconds: llm, total_seconds: stt + (llm ?? 0) },
  };
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
  { id: "hybrid", title: t("Локальное распознавание + LLM"), sub: t("Распознаем локально, затем отправляем текст в LLM для обработки."), icon: "spark" },
  { id: "cloud", title: t("Облачное распознавание"), sub: t("Запись целиком уходит провайдеру и распознаётся у него. Локальная модель не нужна."), icon: "globe" },
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
  onConfigChanged: (change: ConfigChange) => Promise<ConfigResult | null>;
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
  const activeProfile = useMemo(() => activeProfileOf(baseAi, profiles), [baseAi, profiles]);
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
  const [promptSaving, setPromptSaving] = useState(false);
  const [advancedShown, setAdvancedShown] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [testLoading, setTestLoading] = useState(false);
  const [testResult, setTestResult] = useState<AiRunResult | null>(null);
  const [manualText, setManualText] = useState("");
  const [manualLoading, setManualLoading] = useState(false);
  const [manualResult, setManualResult] = useState<AiRunResult | null>(null);
  const {
    fileStage, fileResult, fileError, setFileResult, setFileError,
    transcribeFile, cancelFileTranscription, runFileTranscription,
  } = useFileTranscription();
  const [fileDragActive, setFileDragActive] = useState(false);
  const [fileDetails, setFileDetails] = useState(false);
  const [fileTextStage, setFileTextStage] = useState<TextStage | null>(null);
  const [fileLlmLoading, setFileLlmLoading] = useState(false);
  const [fileLlmError, setFileLlmError] = useState<string | null>(null);
  // The drop subscription is installed once, so busyness is read from refs: a
  // closure over state would freeze the values of the first render.
  const fileStageRef = useRef<FileStage>(null);
  const manualLoadingRef = useRef(false);

  useEffect(() => {
    setPromptDraft(effectiveSystemPrompt(activeProfile));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed by the fields the prompt comes from; the profile object is new on every render.
  }, [activeProfile.id, activeProfile.system_prompt, activeProfile.prompt_preset]);

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
      if (disposed) return;
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
    }).catch((error) => {
      if (!disposed) setFileError(error instanceof Error ? error.message : String(error));
    });
    return () => { disposed = true; unlisten?.(); };
  }, [setFileError, setFileResult, transcribeFile]);

  useEffect(() => { fileStageRef.current = fileStage; }, [fileStage]);
  // Each new file starts with its details folded; the window only hides when
  // closed, so the previous file's choice would otherwise carry over.
  useEffect(() => { if (fileStage !== null) setFileDetails(false); }, [fileStage]);
  // A new result, or the LLM pass over it, opens on its last stage.
  useEffect(() => { setFileTextStage(null); setFileLlmError(null); }, [fileResult]);
  useEffect(() => { manualLoadingRef.current = manualLoading; }, [manualLoading]);

  function showMessage(text: string) {
    setMessage(text);
    window.setTimeout(() => setMessage((current) => (current === text ? null : current)), 3500);
  }

  async function saveAi(patch: Partial<AiConfig>) {
    const editedProfileId = activeProfile.id;
    return onConfigChanged((current) => {
      const currentAi = mergeAi(current.ai_processing ?? null, {});
      const currentProfiles = profilesForAi(currentAi);
      if (patch.active_profile_id) {
        const nextActive = currentProfiles.find((profile) => profile.id === patch.active_profile_id);
        return nextActive ? { ai_processing: activeConfigFromProfile(currentAi, nextActive, currentProfiles) } : {};
      }
      const profileFields = ["model", "base_url", "api_key_ref", "prompt_preset", "system_prompt", "llm_min_duration_seconds", "llm_timeout_seconds", "llm_reasoning", "llm_output_limit"] as const;
      if (!profileFields.some((field) => field in patch)) {
        return { ai_processing: { ...routeFields(currentAi, activeProfileOf(currentAi, currentProfiles), currentProfiles), ...patch } };
      }
      const currentProfile = currentProfiles.find((profile) => profile.id === editedProfileId)
        ?? normalizeProfile(currentAi, { ...activeProfile, id: editedProfileId });
      const updatedProfile = normalizeProfile(currentAi, { ...currentProfile, ...patch });
      const nextProfiles = currentProfiles.map((profile) => profile.id === editedProfileId ? updatedProfile : profile);
      const editsRoute = currentProfiles.length === 0 || currentAi.active_profile_id === editedProfileId;
      const profilePatch: Partial<AiConfig> = {
        ...routeFields(currentAi, editsRoute ? updatedProfile : activeProfileOf(currentAi, nextProfiles), nextProfiles),
        profiles: nextProfiles,
      };
      if (editsRoute && patch.model) {
        profilePatch.provider_models = { ...(currentAi.provider_models ?? {}), [currentProfile.provider]: patch.model };
      }
      return { ai_processing: profilePatch };
    });
  }

  async function savePrompt() {
    const next = promptDraft.trim();
    if (promptSaving || !next || next === (activeProfile.system_prompt ?? "")) return;
    // It matched the built-in one — we save emptiness rather than a copy: a copy
    // freezes and stops receiving edits to the built-in prompt.
    setMessage(null);
    setPromptSaving(true);
    try {
      const saved = await saveAi({ system_prompt: next === builtinPrompt.trim() ? "" : next });
      if (saved) showMessage(t("Системный промпт сохранён."));
    } finally {
      setPromptSaving(false);
    }
  }

  async function resetPrompt() {
    if (promptSaving) return;
    if (!activeProfile.system_prompt?.trim()) {
      setPromptDraft(builtinPrompt);
      return;
    }
    setMessage(null);
    setPromptSaving(true);
    try {
      const saved = await saveAi({ system_prompt: "" });
      if (saved) showMessage(t("Профиль снова использует встроенный промпт."));
    } finally {
      setPromptSaving(false);
    }
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
        llm_reasoning: ai.llm_reasoning,
        llm_output_limit: ai.llm_output_limit,
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
      setManualResult(await processWithTextProfile(text));
    } catch (e) {
      setManualResult({ available: false, message: e instanceof Error ? e.message : String(e) });
    } finally {
      setManualLoading(false);
    }
  }

  /** Run the LLM over a transcribed file in place and record it as the result's
   *  LLM stage, instead of sending the user to paste it into the panel above. */
  async function processFileResult() {
    if (!fileResult || fileLlmLoading) return;
    setFileLlmLoading(true);
    setFileLlmError(null);
    const started = performance.now();
    try {
      const result = await processWithTextProfile(fileResult.formatted_text || fileResult.text);
      if (result.available && result.output && !result.fallback) {
        setFileResult({
          ...fileResult,
          text: result.output,
          ai_status: {
            ...fileResult.ai_status,
            enabled: true, attempted: true, used: true, fallback: false, skipped_reason: "",
            elapsed_seconds: (performance.now() - started) / 1000,
            provider: textProfile.provider, model: textProfile.model, profile_name: textProfile.name,
          },
        });
      } else {
        setFileLlmError(result.provider_error || result.message
          || (result.skipped_reason ? aiFallbackLabel(undefined, result.skipped_reason) : t("LLM не вернула текст.")));
      }
    } catch (e) {
      setFileLlmError(e instanceof Error ? e.message : String(e));
    } finally {
      setFileLlmLoading(false);
    }
  }

  function processWithTextProfile(text: string) {
    // The fields are taken from the profile itself rather than from the flat
    // active ones: under inheritance they are the same, but when a different
    // profile is chosen the flat fields would describe the voice one — that
    // is, they would send the request somewhere other than what is shown.
    return invoke<AiRunResult>("process_text_ai", {
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
        llm_reasoning: textProfile.llm_reasoning,
        llm_output_limit: textProfile.llm_output_limit,
      });
  }

  async function copyFileStage(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      showMessage(t("Результат скопирован."));
    } catch {
      showMessage(t("Не удалось скопировать."));
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

      {message && <div style={{ padding: "10px 12px", borderRadius: "var(--radius-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "500 12px/1.4 var(--font-sans)", marginBottom: 14 }}>{message}</div>}

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
              // The explanation sits in the hover/focus bubble: three paragraphs
              // under the titles made the route the heaviest card on the page.
              return (
                <Hint asChild key={mode.id} text={mode.sub}>
                  <button
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
                  </button>
                </Hint>
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
                  disabled={promptSaving}
                  inlineMeta
                  options={profiles.map((profile) => {
                    const itemProvider = PROVIDERS.find((item) => item.id === profile.provider) ?? PROVIDERS[0];
                    return { value: profile.id, label: profile.name, meta: `${itemProvider.name} · ${profile.model}` };
                  })}
                  onChange={(next) => {
                    if (next === activeProfile.id) return;
                    const picked = profiles.find((profile) => profile.id === next);
                    setMessage(null);
                    void saveAi({ active_profile_id: next }).then((saved) => {
                      if (saved && picked) showMessage(t("Активный профиль: «{p0}».", { p0: picked.name }));
                    });
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
                <h3>{t("Таймаут LLM")}<Hint text={t("Сколько ждать ответа перед вставкой как минимум: для длинного текста время увеличивается само. Если провайдер не успеет, вставится локально обработанный текст, а с ответом поступят по настройке «Поздний ответ LLM». Значение своё у каждого профиля.")}/></h3>
                <NumberField className="mono" min={1} max={60} step={1} value={ai.llm_timeout_seconds ?? 12}
                  onValueChange={(next) => void saveAi({ llm_timeout_seconds: Math.max(1, Math.min(60, Number(next) || 12)) })} style={{ width: 84 }}/>
                <span className="route-advanced__unit">{t("секунд")}</span>
              </div>
              <div className="route-advanced__cell">
                <h3>{t("Рассуждения")}<Hint text={t("Рассуждающие модели думают перед ответом: это время и токены из лимита ответа. Для очистки диктовки рассуждения почти не нужны. «Минимальные» просит модель думать как можно меньше там, где API это позволяет. Значение своё у каждого профиля.")}/></h3>
                <CustomSelect<ReasoningMode>
                  value={ai.llm_reasoning ?? "minimal"}
                  options={[
                    { value: "minimal", label: t("Минимальные") },
                    { value: "model", label: t("Как у модели") },
                  ]}
                  onChange={(next) => void saveAi({ llm_reasoning: next })}
                />
              </div>
              <div className="route-advanced__cell">
                <h3>{t("Лимит ответа")}<Hint text={t("Сколько токенов модель может потратить на один ответ вместе с рассуждениями. «Авто» подбирает лимит по длине текста. Текст, который не помещается в лимит, обрабатывается частями: чем выше лимит, тем крупнее части. Значение своё у каждого профиля.")}/></h3>
                <CustomSelect<"auto" | "unlimited" | "custom">
                  value={typeof ai.llm_output_limit === "number" ? "custom" : (ai.llm_output_limit ?? "auto")}
                  options={[
                    { value: "auto", label: t("Авто") },
                    { value: "unlimited", label: t("Без лимита") },
                    { value: "custom", label: t("Своё значение") },
                  ]}
                  onChange={(next) => void saveAi({ llm_output_limit: next === "custom" ? CUSTOM_OUTPUT_LIMIT_DEFAULT : next })}
                />
                {typeof ai.llm_output_limit === "number" && (
                  <>
                    <NumberField className="mono" min={MIN_OUTPUT_LIMIT} max={200000} step={1024} value={ai.llm_output_limit}
                      aria-label={t("Лимит ответа в токенах")}
                      onValueChange={(next) => void saveAi({ llm_output_limit: Math.max(MIN_OUTPUT_LIMIT, Math.round(Number(next) || CUSTOM_OUTPUT_LIMIT_DEFAULT)) })} style={{ width: 128 }}/>
                    <span className="route-advanced__unit">{t("токенов")}</span>
                  </>
                )}
              </div>
              <div className="route-advanced__cell">
                <h3>{t("Поздний ответ LLM")}<Hint text={t("Если LLM не успела до таймаута, текст уже вставлен без неё. Её ответ может прийти позже, до 5 минут, и заменить текст в истории; в окно он не вставляется. Значение общее для всех профилей.")}/></h3>
                <CustomSelect<LateAnswerMode>
                  value={ai.llm_late_answer ?? "notify"}
                  options={[
                    { value: "notify", label: t("Сохранить в историю и сообщить") },
                    { value: "silent", label: t("Сохранить в историю молча") },
                    { value: "off", label: t("Не ждать") },
                  ]}
                  onChange={(next) => void saveAi({ llm_late_answer: next })}
                />
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
            <div style={{ display: "grid", gap: 6, marginTop: 12, padding: 10, borderRadius: "var(--radius-sm)", background: "var(--bg-2)", border: "1px solid var(--line)" }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span className={testResult.available && !testResult.fallback ? "pill ok" : "pill warn"}>{testResult.available ? t("Ответ получен") : (testResult.fallback ? t("Запрос отправлен, fallback") : t("Запрос не отправлен"))}</span>
                <Hint text={t("Закрыть")} style={{ marginLeft: "auto" }}>
                  <button className="icon-btn" aria-label={t("Закрыть")} onClick={() => setTestResult(null)}><Icon name="x" size={12}/></button>
                </Hint>
              </div>
              {(testResult.message || testResult.provider_error || testResult.skipped_reason) && <div style={{ font: "500 11px/1.45 var(--font-mono)", color: testResult.available ? "var(--ink-mute)" : "var(--err)", whiteSpace: "pre-wrap" }}>{testResult.provider_error || testResult.message || testResult.skipped_reason}</div>}
              <ProviderSnippet result={testResult}/>
              {testResult.output && <div style={{ padding: 10, borderRadius: "var(--radius-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 12px/1.5 var(--font-sans)", whiteSpace: "pre-wrap" }}>{testResult.output}</div>}
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
            hint={t("Все инструкции для модели редактируются здесь. Приложение не добавляет обязательных правил. Исходный текст передаётся отдельно в блоке <dictation>. Сохрани промпт перед проверкой.")}
            actions={
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <span className="picker-label">{t("Пресет")}</span>
                <Segmented
                  disabled={promptSaving}
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
                <div className="flex-row" style={{ gap: 8, flexWrap: "wrap", padding: "7px 10px", borderRadius: "var(--radius-sm)", background: "var(--warn-soft)", border: "1px solid color-mix(in srgb, var(--warn) 30%, transparent)" }}>
                  <Icon name="info" size={13} style={{ color: "var(--warn)", flex: "0 0 auto" }}/>
                  <span style={{ font: "500 11.5px/1.4 var(--font-sans)", color: "var(--ink)" }}>
                    {t("У профиля свой промпт — правки встроенного до него не доходят.")}
                  </span>
                  <button className="btn btn--ghost" type="button" disabled={promptSaving} onClick={() => void resetPrompt()} style={{ height: 24, padding: "0 8px", fontSize: 11, marginLeft: "auto" }}>
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
                aria-label={t("Системный промпт")}
                value={promptDraft}
                disabled={promptSaving}
                onChange={(e) => setPromptDraft(e.target.value)}
                rows={8}
                style={{ width: "100%", resize: "vertical", fontSize: 12 }}
                spellCheck={false}
              />
              <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                <button
                  className="btn btn--primary"
                  onClick={() => void savePrompt()}
                  disabled={promptSaving || !promptDraft.trim() || promptDraft === (activeProfile.system_prompt ?? "")}
                >
                  <Icon name="check" size={12}/>{t("Сохранить промпт")}</button>
                <Hint text={t("Вернуть встроенный промпт и снова получать его правки")}>
                  <button
                    className="btn btn--ghost"
                    onClick={() => void resetPrompt()}
                    disabled={promptSaving || (!promptCustom && promptDraft.trim() === builtinPrompt.trim())}
                  >
                    <Icon name="refresh" size={12}/>{t("Вернуть встроенный")}</button>
                </Hint>
                <span style={{ marginLeft: "auto", font: "500 11px/1 var(--font-mono)", color: "var(--ink-mute)" }}>
                  {promptDraft.length}  {t("симв.")}</span>
              </div>
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
                onChange={(next) => void onConfigChanged({ ai_processing: { text_profile_id: next === activeProfile.id ? "" : next } })}
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
              {fileStage !== null
                ? <button className="btn btn--ghost" onClick={() => void cancelFileTranscription()}>{t("Отменить")}</button>
                : (
                  <button className="btn btn--ghost" onClick={() => void runFileTranscription()} disabled={manualLoading}>
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
              <div style={{ padding: 12, borderRadius: "var(--radius-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 13px/1.55 var(--font-sans)", color: "var(--ink)", whiteSpace: "pre-wrap" }}>{fileResult.text}</div>
              <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
                <button className="btn btn--ghost" onClick={() => setFileDetails((open) => !open)} aria-expanded={fileDetails} style={{ height: 24 }}>
                  <Icon name={fileDetails ? "chev-down" : "chev"} size={11} style={{ transform: fileDetails ? undefined : "rotate(90deg)" }}/>
                  {fileDetails ? t("Скрыть детали") : t("Подробнее")}
                </button>
                {fileResult.text && !fileResult.ai_status?.used && (
                  <button className="btn btn--ghost" onClick={() => void processFileResult()} disabled={fileLlmLoading || !!textGap} aria-busy={fileLlmLoading} style={{ height: 24 }}>
                    {fileLlmLoading ? <span className="mini-spinner" aria-hidden="true"/> : <Icon name="sparkle" size={11}/>}
                    {fileLlmLoading ? t("Обрабатываю…") : t("Обработать через LLM")}
                  </button>
                )}
                <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 6 }}>
                  <FileStatusPill result={fileResult}/>
                  {fileResult.text && (
                    <button className="btn btn--ghost" onClick={() => void copyFileResult()} style={{ height: 24 }}>
                      <Icon name="copy" size={11}/>{t("Скопировать")}
                    </button>
                  )}
                </div>
              </div>
              {fileLlmError && <div role="alert" style={{ font: "500 11px/1.45 var(--font-mono)", color: "var(--err)", whiteSpace: "pre-wrap" }}>{fileLlmError}</div>}
              {fileDetails && (
                <div>
                  <TranscriptStats entry={fileRecord(fileResult)}/>
                  <TranscriptStages
                    entry={fileRecord(fileResult)}
                    copyKey="file"
                    stage={fileTextStage ?? defaultStage(fileRecord(fileResult))}
                    onStage={setFileTextStage}
                    copiedKey={null}
                    onCopy={(_key, text) => void copyFileStage(text)}
                  />
                </div>
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
