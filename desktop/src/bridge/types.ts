export interface AppVersionResult {
  version: string;
}

/** Response from `check_update`. `available: false` means the version is current, not an error. */
export interface UpdateInfo {
  available: boolean;
  current_version: string;
  version?: string | null;
  /** RFC 3339 from the release manifest. */
  date?: string | null;
  /** The "what's new" text from the release notes. */
  notes?: string | null;
}

/** The `update-download-progress` event. `total` is absent without Content-Length. */
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
  /// Names, brands and terms the engine cannot know. On Whisper it goes into
  /// initial_prompt; on any engine it corrects the result.
  custom_words: string[];
  /// Identifiers of the enabled ready-made sets. A list of ids, not a copy of
  /// the words: a set is removed in one motion and does not touch your own words.
  enabled_presets: string[];
}

export interface ConfigResult {
  theme: "dark" | "light";
  /** UI language. Empty or missing — take the system language. Not to be confused with `language`: that one is about speech. */
  ui_language?: "ru" | "en";
  language: string;
  model: string;
  /** "cuda" — a legacy value from old configs, read as "gpu". */
  device: "cpu" | "gpu" | "cuda";
  hotkey: string;
  auto_paste: boolean;
  /** Append a space at the end of the insertion. */
  paste_trailing_space: boolean;
  /** Press Enter after inserting. */
  paste_auto_submit: boolean;
  auto_start: boolean;
  /** Sound cues for the dictation cycle. Enabled by default. */
  sound_feedback: boolean;
  /** Cue volume, 0..1. Default 0.35. */
  sound_volume: number;
  /** Lower the overall volume while recording. Disabled by default. */
  duck_output_while_recording: boolean;
  /** How far to lower it, 0..1. Default 0.2. */
  duck_output_level: number;
  /** Trim silence at the start and end of a recording. Enabled by default. */
  trim_silence: boolean;
  /** How many days to keep history. 0 — no age limit. Default 30. */
  history_retention_days: number;
  /** Cap on the number of history entries. 0 — no limit. Default 1000. */
  history_max_entries: number;
  /** After how many idle minutes the model is unloaded from RAM.
   *  0 — never unload. No field — five minutes: unloading is on by default,
   *  and old configs get it along with the update. */
  model_unload_after_minutes?: number;
  /** Log verbosity. Default "info". */
  log_level: "error" | "warn" | "info" | "debug" | "trace";
  /** Allow collecting and sending product telemetry. Absent means true. */
  telemetry_enabled?: boolean;
  /** Product session inactivity timeout, in minutes. Default 30.
   *  It has no control in the UI: the value is managed by Rust, and the field is
   *  described here so that `save_config` does not drop it when saving. */
  telemetry_session_timeout_minutes?: number;
  /** Save the audio of every recording next to the logs. Disabled by default. */
  debug_save_recordings: boolean;
  /** Issue #24: verbose log of styles and windows around showing/hiding the overlay. */
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
    /** Profile for dictation: voice → whisper → LLM. */
    active_profile_id?: string;
    /**
     * Profile for text pasted by hand on the LLM processing page.
     * Empty or absent means "same as for voice"; old configs land here
     * automatically, which is why there is no migration.
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
     * Key slots not bound to any profile — what the «Добавить ключ» button
     * creates.
     *
     * They are stored here because the key itself lives in the OS store, and
     * that store cannot be enumerated: `has_api_key` can only answer about a
     * known ref. Without this list nobody would query an arbitrary ref after a
     * restart, and the key would vanish from the UI while remaining in
     * Credential Manager.
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
  /** The model is not in memory, but it is selected and downloaded: it will
   *  come back on its own at the next dictation. This is not the same as
   *  "nothing to transcribe with". */
  model_loads_on_demand?: boolean;
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
  /** Breakdown of LLM fallbacks by reason, most frequent first. */
  llm_fallback_reasons?: Array<{
    error_type: string;
    /** 0 — the provider did not answer at all (timeout, dropped connection). */
    http_status: number;
    count: number;
    last_error: string;
    /** YYYY-MM-DD of the most recent occurrence. */
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
  /** The file was found in the models folder rather than downloaded from the catalog: there is nothing to download and nothing to delete. */
  local?: boolean;
  /** Inference engine (`whisper.cpp` or `sherpa-onnx`). */
  engine?: string;
  /** Actual compute backend reported by the engine registry. */
  compute_backend?: string;
  cpu_only?: boolean;
  /** Model family ("Whisper", "GigaAM", …); absent for your own files. */
  family?: string | null;
  /** The model's closed list of languages; absent for multilingual ones. */
  languages?: string[] | null;
  /** Whether the model shows text as the dictation goes. */
  streaming?: boolean;
  /** Weight quantisation: `q8_0`, `int8`, `f16`. Empty for a foreign file. */
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
