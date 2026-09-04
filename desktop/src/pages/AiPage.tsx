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
  /** Первые 500 символов тела ответа. Бэкенд присылал его и раньше, но здесь
   *  поле не было объявлено, и единственный источник правды про «ответ не той
   *  формы» молча терялся между Rust и экраном. */
  response_snippet?: string;
  ai_processing?: { attempted?: boolean; used?: boolean; skipped_reason?: string };
};

/** Стадия обработки прикреплённого файла. Прогресса движок не отдаёт, так
 *  что показать можно только то, на каком из трёх шагов мы стоим. */
type FileStage = null | "decoding" | "transcribing";

/** Ответ `transcribe_audio_file`. Форма другая, чем у `AiRunResult`: там
 *  результат одного вызова LLM, здесь — весь пайплайн распознавания. */
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

/** Пилюля статуса для результата файла.
 *
 *  Отдельно от пилюль «Обработать текст»: там `available === false` значит
 *  «LLM не отработала», и это ошибка. Здесь LLM может не запускаться штатно
 *  — в режиме «только локально» её и не должно быть, — а текст при этом
 *  распознан. Красная пилюля на успешной расшифровке врала бы. */
function FileStatusPill({ result }: { result: TranscribeFileResult }) {
  const ai = result.ai_status;
  if (ai?.fallback) return <span className="pill warn">Fallback</span>;
  if (ai?.used) return <span className="pill ok">{t("Готово")}</span>;
  if (ai?.attempted) return <span className="pill warn">{t("LLM не отработала")}</span>;
  return <span className="pill">{t("Распознано")}</span>;
}

/** Сырой ответ провайдера под ошибкой. Свёрнут: нужен, когда сообщение об
 *  ошибке говорит «не та форма ответа», и не нужен во всех прочих случаях. */
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

/** Что именно мешает выбранному режиму дойти до провайдера. */
function blockerReason(blocker: NonNullable<LlmRouteBlocker>): string {
  if (blocker === "no_profile") return t("Профиля LLM ещё нет: создайте его в «Интеграциях» и сделайте активным.");
  if (blocker === "no_model") return t("У активного профиля не выбрана модель.");
  return t("У активного профиля нет сохранённого API-ключа.");
}

const EMPTY_KEY_INFO: ApiKeyInfo = { available: false, label: "", masked: "" };

type Props = {
  config: AiConfig | null;
  apiKeys: ApiKeyStatus;
  onConfigChanged: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null>;
  onNavigate: (tab: "integrations") => void;
};

/** Ровно тот же список, что в фильтре `pick_audio_file` на стороне Rust. */
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
  // bound to something. It is NOT rendered in the profile list, so a clean
  // install shows an empty list rather than a phantom OpenAI profile.
  const fallbackProfile = useMemo(() => normalizeProfile(baseAi, {}), [baseAi]);
  const activeProfile = profiles.find((item) => item.id === baseAi.active_profile_id) ?? profiles[0] ?? fallbackProfile;
  const ai = useMemo(() => activeConfigFromProfile(baseAi, activeProfile, profiles), [baseAi, activeProfile, profiles]);
  const provider = PROVIDERS.find((item) => item.id === ai.provider) ?? PROVIDERS[0];
  // Режим с LLM, до которой нечем дойти, — молчаливый режим: Rust проставит
  // skipped_reason и вставит локальный текст, а пользователь увидит просто
  // «гибрид не работает». Считаем это здесь, чтобы сказать об этом на экране,
  // где режим и выбирают.
  const routeBlocker = llmRouteBlocker(ai, profiles, apiKeys);

  // Ключ голосового профиля показывает его собственная строка в списке выше
  // («ключ сохранён» / «нет ключа»), поэтому отдельной переменной под него
  // здесь больше нет — панель ручной обработки судит по своему профилю.
  const activeKeyRef = profileKeyRef(activeProfile);

  // Ручная обработка текста может идти другим профилем, чем диктовка: у них
  // разные задачи, и модель, хорошо чистящая речь, не обязана быть лучшей для
  // произвольного текста. По умолчанию наследуется голосовой.
  const textProfile = useMemo(
    () => textProfileFor(baseAi, profiles, activeProfile),
    [baseAi, profiles, activeProfile],
  );
  const textKeyRef = profileKeyRef(textProfile);
  const textKeyInfo = apiKeys[textKeyRef] ?? (textProfile.id === "default" ? apiKeys[textProfile.provider] ?? EMPTY_KEY_INFO : EMPTY_KEY_INFO);
  const textInheritsVoice = textProfile.id === activeProfile.id;

  // Встроенный промпт под пресет профиля и то, что реально уйдёт модели.
  const builtinPrompt = presetPrompt(activeProfile.prompt_preset);
  const promptCustom = promptIsCustom(activeProfile);
  const [promptDraft, setPromptDraft] = useState(effectiveSystemPrompt(activeProfile));
  // Хвост, который Rust дописывает к любому промпту, включая написанный
  // руками. Читаем из бэкенда, а не держим копию здесь: копия разошлась бы
  // с оригиналом на первой же правке, и тогда «что уходит модели» врало бы
  // ровно в том месте, ради которого его показывают.
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
  // Приходит событием в начале сессии, а не в результате: к моменту
  // результата отменять уже нечего.
  const fileSessionId = useRef<number | null>(null);
  const [fileDragActive, setFileDragActive] = useState(false);
  // Подписка на drop ставится один раз, поэтому занятость читается из рефов:
  // замыкание на состоянии заморозило бы значения первого рендера.
  const fileStageRef = useRef<FileStage>(null);
  const manualLoadingRef = useRef(false);
  const skipResetRef = useRef(false);

  useEffect(() => {
    if (skipResetRef.current) { skipResetRef.current = false; return; }
    setPromptDraft(effectiveSystemPrompt(activeProfile));
  }, [activeProfile.id, activeProfile.system_prompt, activeProfile.prompt_preset]);

  useEffect(() => {
    // Не блокирует страницу: пока не приехал, блок просто не показывается.
    invoke<string>("get_output_contract").then(setOutputContract).catch(() => {});
  }, []);

  // Перетаскивание файла в окно.
  //
  // HTML5-события drop сюда не доходят: в webview включён нативный обработчик
  // Tauri, и он их перехватывает. Зато он даёт то, чего DataTransfer дать не
  // может, — путь на диске, а `transcribe_audio_file` принимает именно путь.
  //
  // Событие приходит на всё окно, без привязки к элементу, поэтому зона не
  // ловит попадание курсора, а просто подсвечивается на время перетаскивания:
  // ронять файл больше некуда, и требовать точного прицела было бы придиркой.
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
      // Одна расшифровка за раз: движок всё равно занят, а вторая встала бы
      // в очередь за первой без всякого следа в интерфейсе.
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
    // Совпал со встроенным — сохраняем пустоту, а не копию: копия застынет и
    // перестанет получать правки встроенного промпта.
    await saveAi({ system_prompt: next === builtinPrompt.trim() ? "" : next });
    showMessage(t("Системный промпт сохранён."));
  }

  // Раньше сброс только подставлял текст в поле и ничего не сохранял: пока не
  // нажмёшь ещё и «Сохранить промпт», при следующем заходе возвращался старый.
  // Отсюда и «приходится кликать каждый раз».
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
        // i18n-ignore: образец русской диктовки для пробного запроса в LLM
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
      // Поля берём с самого профиля, а не с плоских активных: при наследовании
      // это одно и то же, а при выборе другого профиля плоские поля описывали
      // бы голосовой — то есть отправляли бы запрос не туда, куда показано.
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
    // Пользователь закрыл диалог — это не ошибка и показывать нечего.
    if (!path) return;
    await transcribeFile(path);
  }

  async function transcribeFile(path: string) {
    setFileError("");
    setFileResult(null);
    setFileStage("decoding");
    // Событие приходит уже после декодирования, но подписку ставим до
    // вызова: иначе гонка между `emit` в Rust и `listen` здесь.
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
             {t("Профили LLM")} <Hint text={t("Один профиль = одна связка provider + ключ + модель. Создаются и редактируются они в «Интеграциях»; здесь профиль только выбирают.")}/>
            <span className="pill mono">{profiles.length}</span>
          </h2>
          <button className="btn btn--ghost" onClick={() => onNavigate("integrations")}>
            <Icon name="server" size={12}/>{t("Управлять профилями")}<Icon name="arrow-right" size={11}/>
          </button>
        </div>
        <div className="profile-list">
            {/* Ссылка на «Интеграции» здесь была третьей подряд: та же кнопка
                стоит в шапке страницы и в заголовке этой карточки. */}
            {profiles.length === 0 && (
              <div style={{ padding: "16px 14px", color: "var(--ink-mute)", font: "400 12px/1.5 var(--font-sans)" }}>
                {t("Профилей пока нет. Настройки LLM ниже применяются к базовой конфигурации; профиль нужен, чтобы хранить несколько связок «провайдер + ключ + модель».")}
              </div>
            )}
            {profiles.map((profile) => {
              const itemProvider = PROVIDERS.find((item) => item.id === profile.provider) ?? PROVIDERS[0];
              const itemKeyInfo = apiKeys[profileKeyRef(profile)] ?? EMPTY_KEY_INFO;
              const selected = profile.id === activeProfile.id;
              return (
                <div
                  key={profile.id}
                  className="profile-row"
                  data-selected={selected}
                  role="button"
                  tabIndex={0}
                  onClick={() => void saveAi({ active_profile_id: profile.id })}
                  onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); void saveAi({ active_profile_id: profile.id }); } }}
                >
                  <span className="dot" style={{ background: itemProvider.dot }}/>
                  <span className="name">{profile.name}</span>
                  <span className="meta">{itemProvider.name} · {profile.model}</span>
                  <span className="key-state" data-ok={itemKeyInfo.available}>
                    {itemKeyInfo.available ? (itemKeyInfo.masked || t("ключ сохранён")) : t("нет ключа")}
                  </span>
                  <span className="activate-cell">
                    {selected ? (
                      <span className="pill accent" style={{ whiteSpace: "nowrap" }}><Icon name="check" size={11}/>  {t("Активный")}</span>
                    ) : (
                      <button
                        className="btn btn--ghost"
                        title={t("Сделать этот профиль активным для pipeline")}
                        onClick={(e) => { e.stopPropagation(); void saveAi({ active_profile_id: profile.id }); showMessage(t("Активный профиль: «{p0}».", { p0: profile.name })); }}
                        style={{ height: 26, whiteSpace: "nowrap" }}
                      >{t("Сделать активным")}</button>
                    )}
                  </span>
                  <span className="actions">
                    {selected && (
                      <button
                        className="btn btn--primary"
                        title={t("Прогнать образец через активный профиль с его промптом и увидеть ответ модели (списывает токены)")}
                        disabled={testLoading || !ai.model.trim()}
                        onClick={(e) => { e.stopPropagation(); void runTestPrompt(); }}
                        style={{ height: 26 }}
                      ><Icon name="spark" size={12}/>{testLoading ? t("Отправляю…") : t("Пробный запрос")}</button>
                    )}
                  </span>
                </div>
              );
            })}
          </div>
          {testResult && (
            <div style={{ display: "grid", gap: 6, marginTop: 12, padding: 10, borderRadius: 8, background: "var(--bg-2)", border: "1px solid var(--line)" }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span className={testResult.available && !testResult.fallback ? "pill ok" : "pill warn"}>{testResult.available ? t("Ответ получен") : (testResult.fallback ? t("Запрос отправлен, fallback") : t("Запрос не отправлен"))}</span>
                <button className="icon-btn" title={t("Закрыть")} onClick={() => setTestResult(null)} style={{ marginLeft: "auto" }}><Icon name="x" size={12}/></button>
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
          {/* Одна строка без заголовка и без кнопки: путь в «Интеграции» уже
              лежит двумя карточками выше, а заголовок повторял бы то же, что
              говорит сама причина. */}
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
              {/* Профиль со своим текстом молча перестаёт получать правки
                  встроенного промпта. Пока это было не видно, конфиг с апреля
                  ходил без правила «не заменяй слова синонимами». */}
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
                    <button
                      key={preset.id}
                      type="button"
                      className={active ? "btn btn--primary" : "btn btn--ghost"}
                      onClick={() => setPromptDraft(preset.prompt)}
                      title={preset.description}
                      style={{ height: 26 }}
                    >
                      {preset.label}
                    </button>
                  );
                })}
                <span style={{ font: "400 11px/1.3 var(--font-sans)", color: "var(--ink-mute)" }}>
                   {t("Кликни пресет, сохрани и протестируй на длинной записи через «Повторить LLM» в истории.")} </span>
              </div>
              {/* Кнопки «развернуть» здесь нет намеренно: окно и так на
                  дюжину строк, а видимый скроллбар показывает, сколько текста
                  осталось, — и высоту всегда можно потянуть за угол. */}
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
                <button
                  className="btn btn--ghost"
                  onClick={() => void resetPrompt()}
                  disabled={!promptCustom && promptDraft.trim() === builtinPrompt.trim()}
                  title={t("Вернуть встроенный промпт и снова получать его правки")}
                >
                  <Icon name="refresh" size={12}/>{t("Вернуть встроенный")} </button>
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
            {/* Раньше здесь было только «по правилам активного профиля» — какой
                моделью, узнать было неоткуда. Селектор одновременно называет
                её и позволяет развести с диктовкой. */}
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
          {/* Своя зона, а не ещё одна кнопка в ряду: выше по карточке — ввод
              текста, здесь — речь, и это два разных входа в обработку. Раньше
              их разделяла только вторая строка кнопок. */}
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
                /* Отмена есть только на этапе распознавания: до него сессии
                   ещё нет, а декодирование заведомо короткое. */
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
              {/* Показываем сырой этап только когда он отличается — иначе это
                  просто вторая копия того же текста. */}
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
