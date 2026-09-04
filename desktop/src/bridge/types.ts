export interface AppVersionResult {
  version: string;
}

/** Ответ `check_update`. `available: false` — актуальная версия, а не ошибка. */
export interface UpdateInfo {
  available: boolean;
  current_version: string;
  version?: string | null;
  /** RFC 3339 из манифеста релиза. */
  date?: string | null;
  /** Текст «что нового» из релизных заметок. */
  notes?: string | null;
}

/** Событие `update-download-progress`. `total` отсутствует без Content-Length. */
export interface UpdateDownloadProgress {
  downloaded: number;
  total?: number | null;
}

export interface TextFormattingConfig {
  enabled: boolean;
  remove_hallucinations: boolean;
  remove_fillers: boolean;
  remove_parasites: boolean;
  remove_duplicates: boolean;
  collapse_phrase_loops: boolean;
  clean_commas: boolean;
  normalize_spaces: boolean;
  split_sentences: boolean;
  capitalize_sentences: boolean;
  final_punctuation: boolean;
  custom_parasite_words: string[];
  /// Имена, бренды и термины, которых движок знать не может. На Whisper
  /// уезжает в initial_prompt, на любом движке — правит результат.
  custom_words: string[];
  /// Идентификаторы включённых готовых наборов. Список id, а не копия
  /// слов: набор снимается одним движением и своих слов не задевает.
  enabled_presets: string[];
}

export interface ConfigResult {
  theme: "dark" | "light";
  /** Язык интерфейса. Пусто/нет поля — брать язык системы. Не путать с `language`: тот про речь. */
  ui_language?: "ru" | "en";
  language: string;
  model: string;
  /** "cuda" — унаследованное значение старых конфигов, читается как "gpu". */
  device: "cpu" | "gpu" | "cuda";
  hotkey: string;
  auto_paste: boolean;
  /** Добавлять пробел в конце вставки. */
  paste_trailing_space: boolean;
  /** Нажимать Enter после вставки. */
  paste_auto_submit: boolean;
  auto_start: boolean;
  /** Звуковые сигналы цикла диктовки. По умолчанию включены. */
  sound_feedback: boolean;
  /** Громкость сигналов, 0..1. По умолчанию 0.35. */
  sound_volume: number;
  /** Убавлять общую громкость на время записи. По умолчанию выключено. */
  duck_output_while_recording: boolean;
  /** До какого уровня убавлять, 0..1. По умолчанию 0.2. */
  duck_output_level: number;
  /** Обрезать тишину в начале и конце записи. По умолчанию включено. */
  trim_silence: boolean;
  /** Сколько дней хранить историю. 0 — без ограничения по возрасту. По умолчанию 30. */
  history_retention_days: number;
  /** Потолок числа записей истории. 0 — без ограничения. По умолчанию 1000. */
  history_max_entries: number;
  /** Подробность логов. По умолчанию "info". */
  log_level: "error" | "warn" | "info" | "debug" | "trace";
  /** Разрешить сбор и отправку продуктовой телеметрии. Отсутствие — true. */
  telemetry_enabled?: boolean;
  /** Таймаут неактивности продуктовой сессии, в минутах. По умолчанию 30.
   *  Ручки в интерфейсе у него нет: значением распоряжается Rust, а поле
   *  описано здесь, чтобы `save_config` не терял его при сохранении. */
  telemetry_session_timeout_minutes?: number;
  /** Сохранять аудио каждой записи рядом с логами. По умолчанию выключено. */
  debug_save_recordings: boolean;
  /** Issue #24: подробный лог стилей и окон вокруг показа/скрытия оверлея. */
  debug_overlay_diag: boolean;
  replacements_paused: boolean;
  recording_mode: "push_to_talk" | "toggle";
  microphone: string | number | null;
  typing_speed_cpm: number;
  replacements: Record<string, string>;
  replacement_rules: ReplacementRule[];
  text_formatting: TextFormattingConfig;
  ai_processing: {
    pipeline_mode: "local" | "hybrid" | "cloud";
    /** Профиль для диктовки: голос → whisper → LLM. */
    active_profile_id?: string;
    /**
     * Профиль для текста, вставленного руками на странице LLM-обработки.
     * Пусто или отсутствует — «как для голоса»; старые конфиги попадают сюда
     * автоматически, поэтому миграции нет.
     */
    text_profile_id?: string;
    provider: string;
    model: string;
    api_key_ref?: string;
    profile_id?: string;
    profile_name?: string;
    provider_models?: Record<string, string>;
    prompt_preset: string;
    spend_limit_usd: number;
    llm_min_duration_seconds?: number;
    llm_timeout_seconds?: number;
    cloud_stt_timeout_seconds?: number;
    system_prompt: string;
    base_url?: string;
    stt_model?: string;
    profiles?: Array<{
      id: string;
      name: string;
      provider: string;
      model: string;
      api_key_ref?: string;
      prompt_preset?: string;
      system_prompt?: string;
      base_url?: string;
      llm_min_duration_seconds?: number;
      llm_timeout_seconds?: number;
    }>;
    /**
     * Слоты ключей, не привязанные ни к одному профилю, — то, что создаёт
     * кнопка «Добавить ключ».
     *
     * Хранятся здесь, потому что сам ключ лежит в хранилище ОС, а оно не
     * перечисляется: `has_api_key` умеет отвечать только про известный ref.
     * Без этого списка произвольный ref после перезапуска никто не опросит,
     * и ключ исчезает из интерфейса, оставаясь в Credential Manager.
     */
    key_slots?: Array<{
      ref: string;
      label: string;
      provider: string;
    }>;
  };
}

export type ReplacementMatchMode = "word" | "phrase" | "contains" | "regex";

export interface ReplacementRule {
  id: string;
  find: string;
  replace: string;
  enabled: boolean;
  match: ReplacementMatchMode;
  case_sensitive: boolean;
  preserve_case: boolean;
  usage_count: number;
}

export interface ReplacementRuleMatch {
  id?: string;
  find?: string;
  replace?: string;
  count: number;
}

export interface RuntimeStatusResult {
  model_loaded: boolean;
  model?: string | null;
  /** Model actually loaded by the engine thread; differs from `model` when a switch failed. */
  loaded_model?: string | null;
  /** Effective STT model for the next recording; in cloud mode this is the remote request model. */
  active_model?: string | null;
  /** Effective STT engine for the next recording (`cloud-stt` for cloud mode). */
  active_engine?: string | null;
  /** Effective compute location for the next recording (`cloud`, `cpu`, or `gpu`). */
  active_device?: string | null;
  device: string | null;
  engine?: string | null;
  cpu_only?: boolean;
  recording: boolean;
  state: string;
  idle_time?: number;
  idle_time_seconds?: number;
  last_error: string | null;
}

export interface StatsResult {
  total_transcriptions: number;
  total_characters: number;
  total_time_saved_seconds: number;
  total_audio_seconds: number;
  total_processing_seconds: number;
  total_whisper_seconds: number;
  total_format_seconds: number;
  total_llm_seconds: number;
  total_llm_attempts: number;
  total_llm_used: number;
  total_llm_fallbacks: number;
  total_llm_input_tokens: number;
  total_llm_output_tokens: number;
  total_llm_tokens: number;
  total_replacement_applications?: number;
  /** Разбивка фолбэков LLM по причинам, по убыванию частоты. */
  llm_fallback_reasons?: Array<{
    error_type: string;
    /** 0 — провайдер не ответил вовсе (таймаут, обрыв связи). */
    http_status: number;
    count: number;
    last_error: string;
    /** YYYY-MM-DD последнего случая. */
    last_seen: string;
  }>;
  daily_history: Array<{
    date: string;
    count: number;
    chars: number;
    time_saved_seconds?: number;
    audio_seconds?: number;
    processing_seconds?: number;
    whisper_seconds?: number;
    format_seconds?: number;
    llm_seconds?: number;
    llm_attempts?: number;
    llm_used?: number;
    llm_fallbacks?: number;
    llm_input_tokens?: number;
    llm_output_tokens?: number;
    llm_tokens?: number;
    replacement_applications?: number;
  }>;
}

export interface MicrophoneResult {
  id?: string | number;
  index?: number;
  name?: string;
  label?: string;
}

export interface ModelInfo {
  id: string;
  label: string;
  size: string;
  ram: string;
  recommended?: boolean;
  downloaded: boolean;
  selected: boolean;
  loaded?: boolean;
  /** Файл найден в папке моделей, а не скачан из каталога: ни скачивать, ни удалять нечего. */
  local?: boolean;
  /** Inference engine (`whisper.cpp` or `sherpa-onnx`). */
  engine?: string;
  /** Actual compute backend reported by the engine registry. */
  compute_backend?: string;
  cpu_only?: boolean;
  /** Семейство модели («Whisper», «GigaAM», …); нет у своих файлов. */
  family?: string | null;
  /** Закрытый список языков модели; отсутствует у многоязычных. */
  languages?: string[] | null;
  /** Показывает ли модель текст по ходу диктовки. */
  streaming?: boolean;
  /** Квантование весов: `q8_0`, `int8`, `f16`. Пусто у чужого файла. */
  quantization?: string | null;
}

export interface ApiKeyInfo {
  available: boolean;
  label: string;
  masked: string;
}

export type ApiKeyStatus = Record<string, ApiKeyInfo>;

export interface PreviewFormatResult {
  original: string;
  formatted: string;
}

export interface HistoryEntry {
  id: number;
  timestamp: number;
  text: string;
  raw_text?: string;
  formatted_text?: string;
  ai_processing?: {
    mode?: string;
    provider?: string;
    model?: string;
    profile_id?: string;
    profile_name?: string;
    api_key_ref?: string;
    enabled?: boolean;
    attempted?: boolean;
    used?: boolean;
    fallback?: boolean;
    skipped_reason?: string;
    error_type?: string;
    provider_error?: string;
    http_status?: number;
    response_snippet?: string;
    audio_duration_seconds?: number;
    min_duration_seconds?: number;
    timeout_seconds?: number;
    attempt_timeout_seconds?: number;
    attempts?: number;
    elapsed_seconds?: number;
    provider_attempts?: Array<{
      attempt?: number;
      elapsed_seconds?: number;
      error_type?: string;
      provider_error?: string;
      http_status?: number;
    }>;
    output_length?: number;
    usage?: {
      input_tokens?: number;
      output_tokens?: number;
      total_tokens?: number;
    };
  };
  processing_stats?: {
    audio_seconds?: number;
    total_seconds?: number;
    whisper_seconds?: number;
    format_seconds?: number;
    llm_seconds?: number;
    replacement_stats?: {
      total?: number;
      pre_llm?: { total?: number; rules?: ReplacementRuleMatch[] };
      post_llm?: { total?: number; rules?: ReplacementRuleMatch[] };
    };
  };
  system_prompt?: string;
  /** Exact primary STT model captured by the engine thread; null for legacy rows. */
  transcription_model?: string | null;
  length: number;
}

export interface HistoryListResult {
  entries: HistoryEntry[];
  max_age_seconds: number;
  max_entries: number;
}

export interface HistoryRetryAiResult {
  updated: boolean;
  entry?: HistoryEntry;
  reason?: string;
}

export interface HistoryUpdateTextResult {
  updated: boolean;
  entry?: HistoryEntry;
  reason?: string;
}

export interface PreviewReplacementsResult {
  original: string;
  result: string;
  applied_count?: number;
  matched_rules?: ReplacementRuleMatch[];
}
