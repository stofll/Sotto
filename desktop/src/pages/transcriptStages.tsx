import type { HistoryEntry } from "../bridge/types";
import { DiffText } from "../components/DiffBlock";
import { Hint } from "../components/Hint";
import { Icon } from "../components/Icon";
import { Segmented } from "../components/Shell";
import { t } from "../i18n";

/** The parts of a transcript both history and file transcription can show:
 *  the stored texts of each stage, the LLM status and the timings. */
export type TranscriptRecord = Pick<HistoryEntry, "text" | "raw_text" | "formatted_text" | "ai_processing" | "processing_stats">;

export function aiStatusKind(entry: TranscriptRecord): "processed" | "fallback" | "skipped" | "none" {
  const ai = entry.ai_processing;
  if (!ai || Object.keys(ai).length === 0) return "none";
  if (ai.attempted && ai.used) return "processed";
  if (ai.attempted && ai.fallback) return "fallback";
  return "skipped";
}

export function aiStatusText(entry: TranscriptRecord): string {
  const ai = entry.ai_processing;
  if (!ai || Object.keys(ai).length === 0) return t("LLM: нет данных");
  if (!ai.enabled) return t("LLM: выключено");
  const profile = ai.profile_name ? `${ai.profile_name} · ` : "";
  const model = `${profile}${[ai.provider, ai.model].filter(Boolean).join(" / ")}`.trim();
  if (ai.attempted && ai.used && ai.accepted_reason != null) return model ? t("LLM: вариант принят вручную · {p0}", { p0: model }) : t("LLM: вариант принят вручную");
  if (ai.attempted && ai.used && ai.parts && ai.parts.used < ai.parts.total) {
    const label = t("LLM: частично · {p0} из {p1} частей", { p0: ai.parts.used, p1: ai.parts.total });
    return model ? `${label} · ${model}` : label;
  }
  if (ai.attempted && ai.used && ai.late) return model ? t("LLM: обработано позже · {p0}", { p0: model }) : t("LLM: обработано позже");
  if (ai.attempted && ai.used) return model ? t("LLM: обработано · {p0}", { p0: model }) : t("LLM: обработано");
  if (ai.attempted && ai.fallback) {
    const label = aiFallbackLabel(ai.error_type, ai.skipped_reason);
    return model ? `LLM: ${label} · ${model}` : `LLM: ${label}`;
  }
  if (ai.skipped_reason === "duration_below_threshold") return t("LLM: пропущено · короче {p0} сек", { p0: Math.round(ai.min_duration_seconds ?? 0) });
  if (ai.skipped_reason === "missing_api_key") return t("LLM: пропущено · нет ключа");
  if (ai.skipped_reason === "missing_provider") return t("LLM: пропущено · нет провайдера");
  if (ai.skipped_reason === "missing_system_prompt") return t("LLM: пропущено · пустой промпт");
  if (ai.skipped_reason === "text_too_long") return t("LLM: пропущено · текст слишком длинный");
  return t("LLM: пропущено");
}

export function aiFallbackLabel(errorType?: string, skippedReason?: string): string {
  // `altered_response` covers several checks; the reason says which one.
  const code = (errorType === "altered_response" ? skippedReason : errorType) || skippedReason || "";
  if (code === "auth_error" || code === "provider_auth_error") return t("ошибка ключа");
  if (code === "rate_limit" || code === "provider_quota_or_rate_limit") return t("лимит");
  if (code === "timeout" || code === "provider_timeout") return "timeout";
  if (code === "connection_error" || code === "provider_connection_error") return t("сеть");
  if (code === "bad_response" || code === "provider_bad_response") return t("неожиданный ответ");
  if (code === "provider_failed") return t("провайдер отклонил запрос");
  if (code === "unknown_provider") return t("неизвестный провайдер");
  if (code === "empty_response") return t("пустой ответ");
  if (code === "meta_response" || code === "model_returned_meta_response") return "meta fallback";
  if (code === "summarised_response" || code === "model_dropped_text") return t("модель сократила текст");
  if (code === "model_dropped_negation") return t("модель убрала отрицание");
  if (code === "model_changed_numbers") return t("модель изменила числа");
  if (code === "model_changed_terms") return t("модель изменила названия");
  if (code === "model_repeated_context") return t("модель повторила контекст");
  // An unmapped code is more useful raw than as the word "fallback".
  return code || "fallback";
}

// Rust returns the raw `skipped_reason` code rather than a sentence, so the
// wording for a given failure lives in exactly one place. The provider-side
// codes are already spelled out by `aiFallbackLabel`; only the gates that
// stop the call before it leaves the app need their own text.
export function aiSkipLabel(code: string): string {
  if (code === "local_mode") return t("режим «локально» — LLM выключена");
  if (code === "missing_provider") return t("не выбран провайдер");
  if (code === "missing_api_key") return t("нет ключа");
  if (code === "missing_system_prompt") return t("пустой системный промпт");
  if (code === "duration_below_threshold") return t("запись короче порога");
  if (code === "text_too_long") return t("текст слишком длинный для LLM");
  return aiFallbackLabel(undefined, code);
}

export function formatSeconds(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "-";
  if (value < 0.1) return t("{p0} мс", { p0: Math.round(value * 1000) });
  return t("{p0} с", { p0: value.toFixed(value < 10 ? 1 : 0) });
}

type Tone = "ok" | "warn" | "mute";
const TONE_COLOR: Record<Tone, string> = { ok: "var(--ok)", warn: "var(--warn)", mute: "var(--ink-faint)" };

function StatTile({ label, value, tone, hint }: { label: string; value: string; tone?: Tone; hint?: string }) {
  const tile = (
    <div style={{
      display: "grid",
      gap: 3,
      padding: "8px 10px",
      background: "var(--bg-3)",
      borderRadius: "var(--radius-sm)",
      border: "1px solid var(--line)",
      minWidth: 0,
      flex: 1,
    }}>
      <span style={{ font: "600 12.5px/1 var(--font-mono)", color: tone ? TONE_COLOR[tone] : "var(--ink)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{value}</span>
      <span style={{ font: "500 9.5px/1 var(--font-mono)", color: "var(--ink-mute)", textTransform: "uppercase", letterSpacing: "0.05em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{label}</span>
    </div>
  );
  return hint ? <Hint text={hint} className="hint-anchor--block">{tile}</Hint> : tile;
}

/** The text before formatting, or `undefined` for entries recorded before the
 *  stages were stored. */
function recognizedText(entry: TranscriptRecord): string | undefined {
  return entry.raw_text || entry.formatted_text || undefined;
}

function formattedText(entry: TranscriptRecord): string {
  return entry.formatted_text || entry.raw_text || entry.text;
}

/** What the LLM step did, as one short tile: its time when it ran, otherwise
 *  why not. The full sentence is the tile's hint. */
function llmTile(entry: TranscriptRecord): { value: string; label: string; tone?: Tone } | null {
  const ai = entry.ai_processing;
  const seconds = entry.processing_stats?.llm_seconds ?? ai?.elapsed_seconds;
  const time = typeof seconds === "number" && Number.isFinite(seconds) ? formatSeconds(seconds) : "—";
  switch (aiStatusKind(entry)) {
    case "none":
      return time === "—" ? null : { value: time, label: "LLM" };
    case "processed":
      return { value: time, label: "LLM" };
    case "fallback":
      return { value: time, label: `LLM · ${aiFallbackLabel(ai?.error_type, ai?.skipped_reason)}`, tone: "warn" };
    case "skipped": {
      const reason = !ai?.enabled || ai.skipped_reason === "local_mode"
        ? t("выкл")
        : ai.skipped_reason === "duration_below_threshold"
          ? t("< {p0} с", { p0: Math.round(ai.min_duration_seconds ?? 0) })
          : null;
      return { value: t("пропуск"), label: reason ? `LLM · ${reason}` : "LLM", tone: "mute" };
    }
  }
}

export function TranscriptStats({ entry }: { entry: TranscriptRecord }) {
  const stats = entry.processing_stats;
  const ai = entry.ai_processing;
  const audioSeconds = stats?.audio_seconds ?? ai?.audio_duration_seconds;
  const seconds = (value: number | undefined) => (typeof value === "number" && Number.isFinite(value) ? formatSeconds(value) : null);
  const recognized = recognizedText(entry);
  const formatChanged = recognized !== undefined && recognized !== formattedText(entry);
  const llm = llmTile(entry);
  const tiles: Array<{ label: string; value: string | null; tone?: Tone; hint?: string }> = [
    { label: t("Аудио"), value: seconds(audioSeconds) },
    { label: "STT", value: seconds(stats?.whisper_seconds) },
    {
      label: t("Формат"),
      value: recognized === undefined ? null : formatChanged ? t("изменён") : t("без изм."),
      tone: formatChanged ? "ok" : "mute",
      hint: formatChanged ? t("Форматирование изменило распознанный текст") : t("Форматирование не меняло распознанный текст"),
    },
    { label: llm?.label ?? "LLM", value: llm?.value ?? null, tone: llm?.tone, hint: aiStatusKind(entry) === "none" ? undefined : aiStatusText(entry) },
    { label: t("Всего"), value: seconds(stats?.total_seconds) },
  ];
  const visible = tiles.filter((tile): tile is typeof tile & { value: string } => tile.value !== null);
  const replacements = stats?.replacement_stats?.total ?? 0;
  const attempts = ai?.attempts && ai.attempts > 1 ? t("попыток {p0}", { p0: ai.attempts }) : null;

  if (visible.length === 0 && !attempts && replacements === 0) return null;

  return (
    <div style={{ marginBottom: 10, display: "grid", gap: 8 }}>
      {visible.length > 0 && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(72px, 1fr))", gap: 6 }}>
          {visible.map(({ label, value, tone, hint }) => (
            <StatTile key={label} label={label} value={value} tone={tone} hint={hint}/>
          ))}
          {replacements > 0 && <StatTile label={t("Замен")} value={String(replacements)}/>}
        </div>
      )}
      {attempts && (
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
          <span className="tag" style={{ height: 20, fontSize: 10 }}>{attempts}</span>
        </div>
      )}
    </div>
  );
}

export type TextStage = "raw" | "formatted" | "llm";

/** The stage a details panel opens on: the last one that produced the text. */
export function defaultStage(entry: TranscriptRecord): TextStage {
  return aiStatusKind(entry) === "processed" ? "llm" : "formatted";
}

/** One recognized text at a time instead of a stack of near-identical blocks.
 *  Formatting and LLM show what they changed against the previous stage; the
 *  copy button always takes the clean text of the stage on screen. */
export function TranscriptStages({ entry, copyKey: keyPrefix, stage, onStage, copiedKey, onCopy }: {
  entry: TranscriptRecord;
  /** Prefix of the copy-button key, unique per transcript on the page. */
  copyKey: string;
  stage: TextStage;
  onStage: (stage: TextStage) => void;
  copiedKey: string | null;
  onCopy: (key: string, text: string) => void;
}) {
  const recognized = recognizedText(entry);
  if (recognized === undefined) return null;
  const formatted = formattedText(entry);
  const llmUsed = aiStatusKind(entry) === "processed";
  const text = stage === "raw" ? recognized : stage === "formatted" ? formatted : llmUsed ? entry.text : null;
  const copyKey = `${keyPrefix}:${stage}`;
  const copied = copiedKey === copyKey;
  const textStyle = { font: "400 13px/1.5 var(--font-sans)", color: "var(--ink)", whiteSpace: "pre-wrap", overflowWrap: "break-word", minWidth: 0 } as const;

  return (
    <div style={{ display: "grid", gap: 8 }}>
      <div>
        <Segmented value={stage} onChange={(value) => onStage(value as TextStage)} options={[
          { value: "raw", label: t("Распознавание") },
          { value: "formatted", label: t("Форматирование") },
          { value: "llm", label: "LLM" },
        ]}/>
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) auto", gap: 8, alignItems: "start", padding: 10, borderRadius: "var(--radius-sm)", background: "var(--bg-3)", border: "1px solid var(--line)" }}>
        {text === null ? (
          <div style={{ ...textStyle, color: "var(--ink-mute)" }}>{aiStatusText(entry)}</div>
        ) : (
          <div style={textStyle}>
            {stage === "raw" ? text : <DiffText before={stage === "formatted" ? recognized : formatted} after={text}/>}
          </div>
        )}
        {text !== null && (
          <Hint text={copied ? t("Скопировано") : t("Скопировать в буфер обмена")}>
            <button
              className={copied ? "btn btn--primary" : "btn btn--ghost"}
              onClick={() => onCopy(copyKey, text)}
              aria-label={t("Копировать текст этапа")}
              style={{ height: 22, padding: "0 6px" }}
            >
              <Icon name={copied ? "check" : "copy"} size={10}/>
            </button>
          </Hint>
        )}
      </div>
    </div>
  );
}
