import { useEffect, useRef, useState, type ReactNode } from "react";
import { invoke, on } from "../bridge";
import type { ConfigResult, PreviewFormatResult, PreviewReplacementsResult, ReplacementMatchMode, ReplacementRule, StatsResult, TextFormattingConfig, UpdateDownloadProgress, UpdateInfo } from "../bridge/types";
import { PageHeader, SectionLabel, Segmented, Switch } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Hint } from "../components/Hint";
import { CustomSelect, type SelectOption } from "../components/CustomSelect";
import { DiffBlock } from "../components/DiffBlock";
import { localeTag, t, tPlural } from "../i18n";
import { DEFAULT_HOTKEY } from "../hotkey";

type StatsRange = "week" | "month" | "year" | "all";
type DailyStats = {
  date: string;
  count: number;
  chars: number;
  time_saved_seconds: number;
  audio_seconds: number;
  processing_seconds: number;
  whisper_seconds: number;
  format_seconds: number;
  llm_seconds: number;
  llm_attempts: number;
  llm_used: number;
  llm_fallbacks: number;
  llm_input_tokens: number;
  llm_output_tokens: number;
  llm_tokens: number;
};

// The keys come from src-tauri/src/ai/step.rs (SKIPPED_REASON_BY_ERROR_TYPE).
// An unknown key is shown as is — a raw name beats an empty string.
const FALLBACK_REASON_LABELS = (): Record<string, string> => ({
  provider_timeout: t("Таймаут провайдера"),
  provider_connection_error: t("Не достучались до провайдера"),
  provider_quota_or_rate_limit: t("Лимит запросов или квота"),
  provider_auth_error: t("Проблема с ключом"),
  provider_bad_response: t("Ответ не разобрался"),
  timeout: t("Таймаут"),
  connection_error: t("Сеть"),
  rate_limit: t("Лимит запросов"),
  auth_error: t("Проблема с ключом"),
  bad_response: t("Ответ не разобрался"),
  unknown: t("Причина не определена"),
});

const RANGE_OPTIONS = () => ([
  { value: "week", label: t("Неделя") },
  { value: "month", label: t("Месяц") },
  { value: "year", label: t("Год") },
  { value: "all", label: t("Всё время") },
]);

function startOfDay(input = new Date()): Date {
  return new Date(input.getFullYear(), input.getMonth(), input.getDate());
}

function addDays(input: Date, days: number): Date {
  const next = new Date(input);
  next.setDate(next.getDate() + days);
  return next;
}

function isoDate(input: Date): string {
  const y = input.getFullYear();
  const m = String(input.getMonth() + 1).padStart(2, "0");
  const d = String(input.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

function shortDateLabel(iso: string): string {
  const [, month, day] = iso.split("-");
  return `${day}.${month}`;
}

function longDateLabel(iso: string): string {
  return new Date(`${iso}T00:00:00`).toLocaleDateString(localeTag(), { day: "numeric", month: "long", weekday: "short" });
}

function emptyDaily(date: string): DailyStats {
  return { date, count: 0, chars: 0, time_saved_seconds: 0, audio_seconds: 0, processing_seconds: 0, whisper_seconds: 0, format_seconds: 0, llm_seconds: 0, llm_attempts: 0, llm_used: 0, llm_fallbacks: 0, llm_input_tokens: 0, llm_output_tokens: 0, llm_tokens: 0 };
}

function formatDuration(seconds: number): string {
  const totalMinutes = Math.round(Math.max(0, seconds) / 60);
  if (totalMinutes < 60) return t("{p0} м", { p0: totalMinutes });
  return t("{p0} ч {p1} м", { p0: Math.floor(totalMinutes / 60), p1: totalMinutes % 60 });
}

function formatShortDuration(seconds: number): string {
  const safe = Math.max(0, seconds);
  if (safe < 60) return t("{p0} с", { p0: safe.toFixed(safe < 10 ? 1 : 0) });
  return formatDuration(safe);
}

function formatSignedDuration(seconds: number): string {
  const prefix = seconds < 0 ? "-" : "";
  return `${prefix}${formatDuration(Math.abs(seconds))}`;
}

function normalizeHistory(stats: StatsResult | null): DailyStats[] {
  return [...(stats?.daily_history ?? [])]
    .filter((item) => /^\d{4}-\d{2}-\d{2}$/.test(item.date))
    .map((item) => ({
      ...emptyDaily(item.date),
      count: Number(item.count) || 0,
      chars: Number(item.chars) || 0,
      time_saved_seconds: Number(item.time_saved_seconds) || 0,
      audio_seconds: Number(item.audio_seconds) || 0,
      processing_seconds: Number(item.processing_seconds) || 0,
      whisper_seconds: Number(item.whisper_seconds) || 0,
      format_seconds: Number(item.format_seconds) || 0,
      llm_seconds: Number(item.llm_seconds) || 0,
      llm_attempts: Number(item.llm_attempts) || 0,
      llm_used: Number(item.llm_used) || 0,
      llm_fallbacks: Number(item.llm_fallbacks) || 0,
      llm_input_tokens: Number(item.llm_input_tokens) || 0,
      llm_output_tokens: Number(item.llm_output_tokens) || 0,
      llm_tokens: Number(item.llm_tokens) || 0,
    }))
    .sort((a, b) => a.date.localeCompare(b.date));
}

function sumDaily(history: DailyStats[], key: keyof Omit<DailyStats, "date">): number {
  return history.reduce((sum, item) => sum + Number(item[key] || 0), 0);
}

function rangeCutoff(range: StatsRange): string | null {
  const today = startOfDay();
  if (range === "week") return isoDate(addDays(today, -6));
  if (range === "month") return isoDate(addDays(today, -29));
  if (range === "year") return isoDate(addDays(today, -364));
  return null;
}

function buildDailySeries(history: DailyStats[], range: StatsRange): DailyStats[] {
  const chartDays = range === "week" ? 7 : range === "month" ? 30 : 84;
  const today = startOfDay();
  const byDate = new Map(history.map((item) => [item.date, item]));
  return Array.from({ length: chartDays }, (_, i) => {
    const date = isoDate(addDays(today, i - chartDays + 1));
    return byDate.get(date) ?? emptyDaily(date);
  });
}

function Heatmap({ history }: { history: DailyStats[] }) {
  const colors = ["var(--surface-2)", "rgba(246,169,59,0.18)", "rgba(246,169,59,0.40)", "rgba(246,169,59,0.65)", "var(--accent)"];
  const cells = buildDailySeries(history, "year");
  const max = Math.max(1, ...cells.map((item) => item.count));
  const monthLabels = [0, 4, 8].map((week) => {
    const cell = cells[Math.min(cells.length - 1, week * 7)];
    return new Date(`${cell.date}T00:00:00`).toLocaleDateString(localeTag(), { month: "short" }).replace(".", "");
  });
  return <div><div style={{ display: "flex", justifyContent: "space-around", marginBottom: 8, paddingLeft: 28 }}>{monthLabels.map((m, i) => <span key={`${m}-${i}`} style={{ font: "500 10px/1 var(--font-mono)", color: "var(--text-mute)" }}>{m}</span>)}</div><div style={{ display: "flex", gap: 8 }}><div style={{ display: "flex", flexDirection: "column", justifyContent: "space-between", font: "500 10px/1 var(--font-mono)", color: "var(--text-mute)", paddingTop: 1, paddingBottom: 1 }}>{[t("Пн"), "", t("Ср"), "", t("Пт"), "", ""].map((d, i) => <span key={i} style={{ height: 12 }}>{d}</span>)}</div><div style={{ display: "grid", gridTemplateColumns: "repeat(12, minmax(10px, 1fr))", gap: 3, flex: 1 }}>{Array.from({ length: 12 }).map((_, w) => <div key={w} className="heatmap-week" style={{ display: "grid", gridTemplateRows: "repeat(7, 1fr)", gap: 3 }}>{Array.from({ length: 7 }).map((_, d) => { const item = cells[w * 7 + d]; const level = item.count === 0 ? 0 : Math.max(1, Math.ceil((item.count / max) * 4)); return <div key={item.date} className="heatmap-cell" style={{ width: "100%", aspectRatio: 1, borderRadius: 3, background: colors[level] }}><div className="heatmap-popover" role="tooltip"><div className="heatmap-popover__title">{longDateLabel(item.date)}</div><div>{item.count.toLocaleString(localeTag())}  {t("распознаваний")}</div><div>{item.chars.toLocaleString(localeTag())}  {t("символов")}</div><div>{t("Аудио:")} {formatShortDuration(item.audio_seconds)}</div><div>{t("Обработка:")} {formatShortDuration(item.processing_seconds)}</div>{item.llm_attempts > 0 && <div>LLM: {item.llm_used}/{item.llm_attempts}  {t("успешно")}</div>}{item.llm_tokens > 0 && <div>{t("Токены:")} {item.llm_tokens.toLocaleString(localeTag())}</div>}</div></div>; })}</div>)}</div></div></div>;
}

function Stat({ label, value, sub, accent = false, hint }: { label: string; value: string; sub: string; accent?: boolean; hint?: string }) {
  return (
    <div className={`stat${accent ? " accent" : ""}`}>
      <div className="stat__label">
        {label}
        {hint && <Hint text={hint}/>}
      </div>
      <div className="stat__value">{value}</div>
      <div className="stat__sub">{sub}</div>
    </div>
  );
}

function BreakdownRow({ label, value, tone }: { label: string; value: string; tone?: "accent" | "info" | "ok" }) {
  return (
    <div className="breakdown-row">
      <span className="breakdown-row__label">{label}</span>
      <span className={`breakdown-row__value${tone ? ` tone-${tone}` : ""}`}>{value}</span>
    </div>
  );
}

function LineChart({ data }: { data: DailyStats[] }) {
  const maxY = Math.max(0.01, ...data.flatMap((d) => [d.whisper_seconds, d.llm_seconds]));
  const n = Math.max(1, data.length - 1);
  const mkPath = (key: "whisper_seconds" | "llm_seconds") =>
    data.map((d, i) => {
      const x = (i / n) * 100;
      const y = 100 - (Number(d[key] || 0) / maxY) * 90;
      return `${i === 0 ? "M" : "L"} ${x.toFixed(2)} ${y.toFixed(2)}`;
    }).join(" ");
  const whisperPath = mkPath("whisper_seconds");
  const llmPath = mkPath("llm_seconds");
  return (
    <svg viewBox="0 0 100 100" preserveAspectRatio="none" style={{ width: "100%", height: 200, overflow: "visible" }}>
      {[0, 25, 50, 75, 100].map((y) => (
        <line key={y} x1="0" y1={y} x2="100" y2={y} stroke="var(--line-soft)" strokeWidth="0.2" vectorEffect="non-scaling-stroke"/>
      ))}
      <path d={`${whisperPath} L 100 100 L 0 100 Z`} fill="var(--accent-soft)" opacity="0.6"/>
      <path d={whisperPath} fill="none" stroke="var(--accent)" strokeWidth="1.6" vectorEffect="non-scaling-stroke"/>
      <path d={llmPath} fill="none" stroke="var(--info)" strokeWidth="1.6" vectorEffect="non-scaling-stroke" strokeDasharray="3 2"/>
    </svg>
  );
}

function BarChart({ data }: { data: DailyStats[] }) {
  const max = Math.max(1, ...data.map((d) => d.count));
  return (
    <div className="bar-row">
      {data.map((d) => {
        const height = d.count === 0 ? 6 : Math.max(6, (d.count / max) * 100);
        // The accent marks days with entries. The last five columns used to be
        // highlighted regardless of the data — on empty statistics that drew
        // activity which never happened.
        return <div key={d.date} className={`bar-row__cell${d.count > 0 ? " active" : ""}`} title={`${shortDateLabel(d.date)}: ${d.count}`} style={{ height: `${height}%` }}/>;
      })}
    </div>
  );
}

export function StatsPage({ stats, typingSpeedCpm = 240, onRefresh }: { stats: StatsResult | null; typingSpeedCpm?: number; onRefresh?: () => Promise<void> }) {
  const [range, setRange] = useState<StatsRange>("month");
  const [refreshing, setRefreshing] = useState(false);
  const history = normalizeHistory(stats);
  const cutoff = rangeCutoff(range);
  const rangeHistory = cutoff ? history.filter((item) => item.date >= cutoff) : history;
  const dailySeries = buildDailySeries(history, range);
  const speedCpm = Math.max(1, Number(typingSpeedCpm) || 240);

  // A single place on the page where the data source is chosen.
  //
  // The large number always used to come from lifetime counters while the
  // filter changed only the small caption beneath it — hence "I set a filter and
  // half the numbers do not move". Now the period picks the source outright: the
  // counters answer for "all time", the daily sums for every other period.
  //
  // They must not be mixed in one row: the counters count everything while
  // stats_daily keeps a year, and subtracting one from the other is nonsense.
  const allTime = range === "all";
  const pick = (total: number | undefined, key: keyof Omit<DailyStats, "date">): number =>
    allTime ? (total ?? 0) : sumDaily(rangeHistory, key);

  const transcriptions = pick(stats?.total_transcriptions, "count");
  const chars = pick(stats?.total_characters, "chars");
  const audioSeconds = pick(stats?.total_audio_seconds, "audio_seconds");
  const processingSeconds = pick(stats?.total_processing_seconds, "processing_seconds");
  const whisperSeconds = pick(stats?.total_whisper_seconds, "whisper_seconds");
  const formatSeconds = pick(stats?.total_format_seconds, "format_seconds");
  const llmSeconds = pick(stats?.total_llm_seconds, "llm_seconds");
  const llmAttempts = pick(stats?.total_llm_attempts, "llm_attempts");
  const llmUsed = pick(stats?.total_llm_used, "llm_used");
  const llmFallbacks = pick(stats?.total_llm_fallbacks, "llm_fallbacks");
  const llmTokens = pick(stats?.total_llm_tokens, "llm_tokens");
  const llmInputTokens = pick(stats?.total_llm_input_tokens, "llm_input_tokens");
  const llmOutputTokens = pick(stats?.total_llm_output_tokens, "llm_output_tokens");
  // The backend reports fallback reasons only as a total across all days, with
  // no per-date breakdown — labelling them "all time" is more honest than
  // pretending they obey the filter.
  const fallbackReasons = stats?.llm_fallback_reasons ?? [];

  const manualTypingSeconds = chars / speedCpm * 60;
  const netSavedSeconds = manualTypingSeconds - audioSeconds - processingSeconds;
  const activeDays = rangeHistory.filter((item) => item.count > 0).length;
  const averageChars = transcriptions > 0 ? Math.round(chars / transcriptions) : 0;
  const averageProcessing = transcriptions > 0 ? processingSeconds / transcriptions : 0;
  const realtimeFactor = processingSeconds > 0 && audioSeconds > 0 ? audioSeconds / processingSeconds : 0;
  const rangeLabel = RANGE_OPTIONS().find((item) => item.value === range)?.label.toLowerCase() ?? t("период");
  // One period caption for every card: now that the number depends on the
  // filter, that has to be said on the card itself, not only in the switch.
  const periodSub = allTime ? t("за всё время") : t("за {p0}", { p0: rangeLabel });
  const axisLabels = dailySeries.length > 0 ? (() => {
    const idxs = [0, Math.floor(dailySeries.length / 4), Math.floor(dailySeries.length / 2), Math.floor((3 * dailySeries.length) / 4), dailySeries.length - 1];
    return Array.from(new Set(idxs)).map((i) => shortDateLabel(dailySeries[i].date));
  })() : [];
  const barAxisLabels = dailySeries.length > 0 ? [shortDateLabel(dailySeries[0].date), shortDateLabel(dailySeries[Math.floor(dailySeries.length / 2)].date), shortDateLabel(dailySeries[dailySeries.length - 1].date)] : [];

  async function refresh() {
    if (!onRefresh) return;
    setRefreshing(true);
    try {
      await onRefresh();
    } finally {
      setRefreshing(false);
    }
  }

  return (
    <div className="page">
      <PageHeader
        title={t("Статистика")}
        actions={<>
          <span className="head-count" title={t("Скорость ручного набора из настроек")}>{speedCpm.toLocaleString(localeTag())}  {t("симв/мин")}</span>
          <Segmented value={range} options={RANGE_OPTIONS()} onChange={(value) => setRange(value as StatsRange)}/>
          <button className="btn btn--ghost" onClick={() => void refresh()} disabled={refreshing}><Icon name="refresh" size={13}/>{refreshing ? t("Обновляю") : t("Обновить")}</button>
        </>}
      />

      <div className="stats-grid">
        <Stat label={t("Распознаваний")} value={transcriptions.toLocaleString(localeTag())} sub={periodSub}/>
        <Stat label={t("Символов")} value={chars.toLocaleString(localeTag())} sub={t("в среднем {p0} на запись", { p0: averageChars.toLocaleString(localeTag()) })}/>
        <Stat label={t("Ручной набор")} value={formatDuration(manualTypingSeconds)} sub={t("Символы / {p0} симв/мин.", { p0: speedCpm.toLocaleString(localeTag()) })}/>
        <Stat label={t("Чистая экономия")} value={formatSignedDuration(netSavedSeconds)} sub={t("минус аудио и обработка")} accent={netSavedSeconds >= 0} hint={t("Оценка ручного набора минус длительность аудио и обработка.")}/>
        <Stat label={t("Активных дней")} value={String(activeDays)} sub={t("{p0} дней сохранено в истории", { p0: history.length })}/>
        <Stat label={t("Аудио")} value={formatDuration(audioSeconds)} sub={periodSub} hint={t("Суммарная длительность записанных фрагментов.")}/>
        <Stat label={t("Обработка")} value={formatShortDuration(processingSeconds)} sub={t("{p0} на запись", { p0: formatShortDuration(averageProcessing) })} hint={t("STT {p0} + форматирование {p1} + LLM {p2}.", { p0: formatShortDuration(whisperSeconds), p1: formatShortDuration(formatSeconds), p2: formatShortDuration(llmSeconds) })}/>
        <Stat label="LLM" value={`${llmUsed.toLocaleString(localeTag())}/${llmAttempts.toLocaleString(localeTag())}`} sub={t("успешно, fallback: {p0}", { p0: llmFallbacks.toLocaleString(localeTag()) })}/>
        <Stat label={t("Токены")} value={llmTokens > 0 ? llmTokens.toLocaleString(localeTag()) : "—"} sub={llmTokens > 0 ? t("вход {p0} / выход {p1}", { p0: llmInputTokens.toLocaleString(localeTag()), p1: llmOutputTokens.toLocaleString(localeTag()) }) : t("появятся после ответа provider usage")}/>
        <Stat label={t("Скорость")} value={realtimeFactor > 0 ? `${realtimeFactor.toFixed(1)}×` : "—"} sub={t("аудио / полная обработка")}/>
      </div>

      <div className="stats-charts">
        <section className="card chart-card">
          <div className="chart-card__head">
            <div>
              <div className="chart-card__title">{t("Время этапа по дням")}</div>
              <div className="chart-card__sub">{t("STT и LLM — секунды")}</div>
            </div>
            <div className="chart-legend">
              <span className="chart-legend__item"><span className="chart-legend__swatch" style={{ background: "var(--accent)" }}/> STT</span>
              <span className="chart-legend__item"><span className="chart-legend__swatch" style={{ background: "var(--info)" }}/> LLM</span>
            </div>
          </div>
          <LineChart data={dailySeries}/>
          <div className="chart-axis">
            {axisLabels.map((label, i) => <span key={`${label}-${i}`}>{label}</span>)}
          </div>
        </section>

        <section className="card chart-card">
          <div className="chart-card__title">{t("Распознаваний по дням")}</div>
          <div className="chart-card__sub" style={{ marginBottom: 14 }}>{dailySeries.length} {tPlural(dailySeries.length, ["день", "дня", "дней"])}</div>
          <BarChart data={dailySeries}/>
          <div className="chart-axis">
            {barAxisLabels.map((label, i) => <span key={`${label}-${i}`}>{label}</span>)}
          </div>
        </section>
      </div>

      <div className="stats-bottom">
        <section className="card chart-card">
          <div className="chart-card__head">
            <div>
              <div className="chart-card__title">{t("Активность за 12 недель")}</div>
              <div className="chart-card__sub">{t("Заполненность ячейки = объём диктовки")}</div>
            </div>
            <span style={{ font: "500 10.5px/1 var(--font-mono)", color: "var(--ink-mute)" }}>{t("наведите на ячейку")}</span>
          </div>
          <Heatmap history={history}/>
        </section>

        <section className="card chart-card">
          <div className="chart-card__head">
            <div className="chart-card__title">{t("Разбивка обработки")}</div>
            <span className="head-count">{periodSub}</span>
          </div>
          <BreakdownRow label="STT" value={formatShortDuration(whisperSeconds)} tone="accent"/>
          <BreakdownRow label={t("Форматирование")} value={formatShortDuration(formatSeconds)}/>
          <BreakdownRow label="LLM" value={formatShortDuration(llmSeconds)} tone="info"/>
          <BreakdownRow label={t("Input токены")} value={llmInputTokens.toLocaleString(localeTag())}/>
          <BreakdownRow label={t("Output токены")} value={llmOutputTokens.toLocaleString(localeTag())}/>
          <BreakdownRow label="Fallback LLM" value={llmFallbacks.toLocaleString(localeTag())} tone={llmFallbacks === 0 ? "ok" : undefined}/>
          {/* The "all time" duplicate is gone: the row used to mix lifetime
              numbers into a card covering a period; now all time is simply
              another position of the filter. */}
          <div className="breakdown-note">
             {t("Стоимость в деньгах не считается без таблицы тарифов; сохраняются только usage-токены провайдера.")} </div>
        </section>
      </div>

      {fallbackReasons.length > 0 && (
        <section className="card chart-card" style={{ marginTop: 12 }}>
          <div className="chart-card__head">
            <div>
              <div className="chart-card__title">{t("Почему LLM отваливался")}</div>
              <div className="chart-card__sub">{t("Счётчик по причинам живёт дольше истории, поэтому разбор не упирается в срок хранения записей")}</div>
            </div>
            {/* The only block that does not obey the filter: the database has
                no per-date breakdown of the reasons. Hence the "all time"
                caption. */}
            <span className="head-count">{fallbackReasons.reduce((sum, reason) => sum + reason.count, 0).toLocaleString(localeTag())}  {t("за всё время")}</span>
          </div>
          {fallbackReasons.map((reason) => (
            <BreakdownRow
              key={`${reason.error_type}-${reason.http_status}`}
              label={`${FALLBACK_REASON_LABELS()[reason.error_type] ?? reason.error_type}${reason.http_status ? ` · HTTP ${reason.http_status}` : ""}`}
              value={`${reason.count.toLocaleString(localeTag())} · ${reason.last_seen}`}
            />
          ))}
          {fallbackReasons.some((reason) => reason.last_error) && (
            <div className="breakdown-note">
               {t("Последняя ошибка:")} {fallbackReasons.find((reason) => reason.last_error)?.last_error}
            </div>
          )}
        </section>
      )}
    </div>
  );
}

const MATCH_LABELS = (): Record<string, string> => ({ word: t("Слово"), phrase: t("Фраза"), contains: t("Внутри"), regex: "Regex" });

function makeReplacementRule(find = "", replace = ""): ReplacementRule {
  const id = typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : `rule-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return { id, find, replace, enabled: true, match: "word", case_sensitive: false, preserve_case: false, usage_count: 0 };
}

function replacementRulesFromConfig(config: ConfigResult | null): ReplacementRule[] {
  const rules = Array.isArray(config?.replacement_rules) ? config!.replacement_rules : [];
  if (rules.length) return rules.map((rule) => ({ ...makeReplacementRule(), ...rule, find: rule.find ?? "", replace: rule.replace ?? "" }));
  return Object.entries(config?.replacements ?? {}).map(([find, replace]) => ({ ...makeReplacementRule(find, replace), id: `legacy-${find}` }));
}

function replacementRulesToLegacyRecord(rules: ReplacementRule[]): Record<string, string> {
  const result: Record<string, string> = {};
  for (const rule of rules) {
    const find = rule.find.trim();
    if (find && rule.enabled) result[find] = rule.replace;
  }
  return result;
}

function validateReplacementRules(rules: ReplacementRule[]): string | null {
  const seen = new Set<string>();
  for (const rule of rules) {
    const find = rule.find.trim();
    if (!find) return t("У каждого правила должно быть заполнено поле поиска.");
    const key = `${rule.match}:${find.toLowerCase()}`;
    if (seen.has(key)) return t("Правила с одинаковым поиском и режимом совпадения конфликтуют между собой.");
    seen.add(key);
    if (rule.find === rule.replace && rule.match !== "regex") return t("Одно из правил ничего не меняет: текст поиска совпадает с заменой.");
  }
  return null;
}

function downloadTextFile(filename: string, text: string, mime = "application/json;charset=utf-8") {
  const blob = new Blob([text], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function ReplacementsEmptyState() {
  return (
    <div style={{ display: "grid", placeItems: "center", padding: "46px 24px", color: "var(--text-mute)", textAlign: "center" }}>
      <div style={{ marginBottom: 12, opacity: 0.75 }}><Icon name="replace" size={32}/></div>
      <div style={{ font: "500 14px/1.3 var(--font-sans)", color: "var(--text-2)" }}>{t("Нет правил замены")}</div>
      <div style={{ marginTop: 6, maxWidth: 390, font: "400 12px/1.5 var(--font-sans)" }}>{t("Добавьте слово или фразу, которые нужно автоматически исправлять после распознавания.")}</div>
    </div>
  );
}

const FORMAT_DEFAULTS: TextFormattingConfig = {
  enabled: true,
  remove_hallucinations: true,
  remove_fillers: true,
  remove_parasites: true,
  remove_duplicates: true,
  collapse_phrase_loops: true,
  clean_commas: true,
  normalize_spaces: true,
  split_sentences: false,
  capitalize_sentences: true,
  final_punctuation: true,
  custom_parasite_words: [],
  custom_words: [],
  enabled_presets: [],
};

type FormatRule = { key: keyof TextFormattingConfig; title: string; sub: string };

// The master switch for the whole local pass stands apart from the list: it
// goes into the header of the «Очистка» card rather than into its body.
const MASTER_RULE = (): FormatRule => (
  { key: "enabled", title: t("Включить форматирование"), sub: t("Главный переключатель всего локального пайплайна") }
);

const CLEAN_RULES = (): FormatRule[] => ([
  { key: "remove_hallucinations", title: t("Убирать артефакты распознавания"), sub: t("«субтитры сделал…», «спасибо за просмотр», [Music]; если кроме них ничего нет — вставка отменяется") },
  { key: "remove_fillers", title: t("Удалять заполнители"), sub: t("э-э, ммм, а-а и похожие звуки") },
  { key: "remove_parasites", title: t("Удалять слова-паразиты"), sub: t("ну, типа, как бы, в общем и свои слова ниже") },
  { key: "remove_duplicates", title: t("Удалять повторы"), sub: t("я я хочу -> я хочу") },
  { key: "collapse_phrase_loops", title: t("Схлопывать зациклившиеся фразы"), sub: t("я думаю что. я думаю что. я думаю что. -> я думаю что.") },
  { key: "clean_commas", title: t("Чистить запятые"), sub: t("лишние запятые перед и/а/но, двойные запятые") },
  { key: "normalize_spaces", title: t("Нормализовать пробелы"), sub: t("двойные пробелы и пробелы перед знаками") },
  { key: "split_sentences", title: t("Разбивать длинные предложения"), sub: t("мягкое разделение длинных фраз по связкам") },
  { key: "capitalize_sentences", title: t("Капитализация предложений"), sub: t("заглавная буква в начале текста и после точки") },
  { key: "final_punctuation", title: t("Финальная пунктуация"), sub: t("добавлять точку, если фраза без знака в конце") },
]);

function normalizeTextFormatting(config: ConfigResult | null): TextFormattingConfig {
  return { ...FORMAT_DEFAULTS, ...(config?.text_formatting ?? {}) };
}

function parseCustomWords(value: string): string[] {
  return value.split(/[\n,]/).map((item) => item.trim()).filter(Boolean);
}

// A versioned key: the default changed to "everything collapsed", and a saved
// choice under the old key would have overridden it for everyone who had already
// opened this page. Resetting the folds once is cheaper than a default nobody
// ever sees.
const TEXT_FOLDS_KEY = "sotto.text.folds.v2";

/** A collapsible card on the «Текст» page.
 *
 * The header is split into two zones: the collapse button (icon, name, summary)
 * and an `aside` beside it. The switches live in `aside` on purpose — inside the
 * button a click on a switch would collapse the card as well. */
function Foldable({ open, title, summary, aside, onToggle, children }: { open: boolean; title: string; summary?: ReactNode; aside?: ReactNode; onToggle: () => void; children: ReactNode }) {
  return (
    <section className="card fold">
      <div className="fold__head">
        <button type="button" className="fold__toggle" onClick={onToggle} aria-expanded={open}>
          <span className="fold__chev" data-open={open ? "true" : "false"}><Icon name="chev-right" size={13}/></span>
          <span className="fold__title">{title}</span>
          {summary}
        </button>
        {aside && <div className="fold__aside">{aside}</div>}
      </div>
      {open && <div className="fold__body">{children}</div>}
    </section>
  );
}

/** «Обработка → Текст»: the entire local pass — cleanup, replacements,
 * dictionaries.
 *
 * These used to be two pages, «Форматирование» and «Замены». The split was a
 * fiction: in the backend one `Formatter::process` runs both halves, and
 * `preview_format` already applied the replacements — that is, the preview on
 * «Форматирование» showed a result its own switches did not explain. Here there
 * is one pass and one preview. */
export function TextPage({ config, onConfigChanged }: { config: ConfigResult | null; onConfigChanged: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null> }) {
  // ── Cleanup and dictionaries: saved immediately, no draft ──────────────
  const formatting = normalizeTextFormatting(config);
  const [customWordsText, setCustomWordsText] = useState(formatting.custom_parasite_words.join("\n"));
  const [dictionaryText, setDictionaryText] = useState(formatting.custom_words.join("\n"));
  const [presets, setPresets] = useState<[string, string[]][]>([]);

  // ── Replacements: a draft until the «Сохранить» button ─────────────────
  const configRules = replacementRulesFromConfig(config);
  const paused = config?.replacements_paused ?? false;
  const [rules, setRules] = useState<ReplacementRule[]>(() => configRules);
  const [filter, setFilter] = useState("");
  const [formError, setFormError] = useState<string | null>(null);
  const [savingRules, setSavingRules] = useState(false);
  const [saved, setSaved] = useState(true);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const findRef = useRef<HTMLInputElement>(null);

  // ── A shared preview of the whole local pass ───────────────────────────
  // i18n-ignore: a Russian dictation sample showing cleanup and replacements
  const [previewText, setPreviewText] = useState("эээ ну в общем, я я хочу сказать что тайпскрипт мы обсудим в понед и я потом отправлю мой мейл");
  const [previewResult, setPreviewResult] = useState("");
  const [previewMatches, setPreviewMatches] = useState<PreviewReplacementsResult["matched_rules"]>([]);
  const [previewError, setPreviewError] = useState<string | null>(null);

  const [folds, setFolds] = useState<Record<string, boolean>>(() => {
    try {
      const stored = window.localStorage.getItem(TEXT_FOLDS_KEY);
      if (stored) return JSON.parse(stored) as Record<string, boolean>;
    } catch {/* ignore */}
    // The page opens as a list of what it contains rather than the expanded
    // contents of two blocks: with «Очистка» and «Замены» expanded you had to
    // scroll to reach the preview on the right.
    return { clean: false, repl: false, dict: false };
  });

  function toggleFold(id: string) {
    setFolds((current) => {
      const next = { ...current, [id]: !current[id] };
      try { window.localStorage.setItem(TEXT_FOLDS_KEY, JSON.stringify(next)); } catch {/* ignore */}
      return next;
    });
  }

  useEffect(() => {
    setCustomWordsText(formatting.custom_parasite_words.join("\n"));
  }, [formatting.custom_parasite_words.join("\n")]);

  useEffect(() => {
    setDictionaryText(formatting.custom_words.join("\n"));
  }, [formatting.custom_words.join("\n")]);

  useEffect(() => {
    void invoke<[string, string[]][]>("dictionary_presets")
      .then(setPresets)
      .catch(() => setPresets([]));
  }, []);

  useEffect(() => {
    setRules(configRules);
    setSaved(true);
  }, [JSON.stringify(config?.replacement_rules ?? []), JSON.stringify(config?.replacements ?? {})]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    on<{ find: string; replace?: string }>("prefill-replacement", (payload) => {
      const find = (payload?.find ?? "").trim();
      if (!find) return;
      setRules((current) => [makeReplacementRule(find, payload.replace ?? ""), ...current]);
      setSaved(false);
      // The rule arrived from the history — the replacements card may be
      // collapsed, and the draft would end up out of sight.
      setFolds((current) => ({ ...current, repl: true }));
      window.setTimeout(() => findRef.current?.focus(), 0);
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, []);

  // Two calls for one preview: `preview_format` gives the full local result
  // (cleanup AND replacements at once), while `preview_replacements` gives only
  // metadata about which rules fired. The first does not return that.
  useEffect(() => {
    let cancelled = false;
    const timer = window.setTimeout(() => {
      const patch = { replacement_rules: rules };
      Promise.all([
        invoke<PreviewFormatResult>("preview_format", { text: previewText, patch }),
        invoke<PreviewReplacementsResult>("preview_replacements", { text: previewText, patch }),
      ])
        .then(([format, replacements]) => {
          if (cancelled) return;
          setPreviewResult(format.formatted || "");
          setPreviewMatches(replacements.matched_rules ?? []);
          setPreviewError(null);
        })
        .catch((e) => {
          if (cancelled) return;
          setPreviewResult("");
          setPreviewMatches([]);
          setPreviewError(e instanceof Error ? e.message : String(e));
        });
    }, 180);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [previewText, JSON.stringify(rules), JSON.stringify(formatting), paused]);

  // The cleanup settings save themselves, with no button and no indicator: the
  // checkbox is the confirmation — it stays in its new position once the config
  // comes back. A separate "saving" pill used to live in the page header and
  // flashed at every sneeze.
  async function saveFormatting(patch: Partial<TextFormattingConfig>) {
    await onConfigChanged({ text_formatting: patch as TextFormattingConfig });
  }

  function saveCustomWords() {
    void saveFormatting({ custom_parasite_words: parseCustomWords(customWordsText) });
  }

  function saveDictionary() {
    void saveFormatting({ custom_words: parseCustomWords(dictionaryText) });
  }

  // A set is stored as an identifier rather than a copy of the words: turning it
  // off is instant and lossless, and it does not touch the field with your own
  // words at all.
  function togglePreset(id: string) {
    const on = formatting.enabled_presets ?? [];
    const next = on.includes(id) ? on.filter((x) => x !== id) : [...on, id];
    void saveFormatting({ enabled_presets: next });
  }

  function updateRule(id: string, patch: Partial<ReplacementRule>) {
    setRules((current) => current.map((rule) => rule.id === id ? { ...rule, ...patch } : rule));
    setSaved(false);
  }

  function moveRule(id: string, direction: -1 | 1) {
    setRules((current) => {
      const index = current.findIndex((rule) => rule.id === id);
      const nextIndex = index + direction;
      if (index < 0 || nextIndex < 0 || nextIndex >= current.length) return current;
      const next = [...current];
      [next[index], next[nextIndex]] = [next[nextIndex], next[index]];
      return next;
    });
    setSaved(false);
  }

  function deleteRule(id: string) {
    setRules((current) => current.filter((rule) => rule.id !== id));
    setSaved(false);
  }

  async function saveRules(nextRules = rules) {
    const validation = validateReplacementRules(nextRules);
    if (validation) {
      setFormError(validation);
      return;
    }
    setFormError(null);
    setSavingRules(true);
    try {
      const result = await onConfigChanged({ replacement_rules: nextRules, replacements: replacementRulesToLegacyRecord(nextRules) });
      if (result) setRules(replacementRulesFromConfig(result));
      setSaved(true);
    } catch {
      setFormError(t("Не удалось сохранить правила замен."));
    } finally {
      setSavingRules(false);
    }
  }

  async function setPaused(nextPaused: boolean) {
    setSavingRules(true);
    try {
      await onConfigChanged({ replacements_paused: nextPaused });
    } finally {
      setSavingRules(false);
    }
  }

  function addRule(find = "", replace = "") {
    setRules((current) => [makeReplacementRule(find, replace), ...current]);
    setSaved(false);
    setFolds((current) => ({ ...current, repl: true }));
    window.setTimeout(() => findRef.current?.focus(), 0);
  }

  function exportRules() {
    downloadTextFile("replacement-rules.json", JSON.stringify({ replacement_rules: rules }, null, 2));
  }

  async function importRules(file: File) {
    try {
      const parsed = JSON.parse(await file.text());
      const imported = Array.isArray(parsed?.replacement_rules)
        ? parsed.replacement_rules.map((rule: Partial<ReplacementRule>) => ({ ...makeReplacementRule(), ...rule }))
        : Object.entries(parsed as Record<string, string>).map(([find, replace]) => makeReplacementRule(find, String(replace)));
      const next = [...rules, ...imported].filter((rule) => rule.find.trim());
      const validation = validateReplacementRules(next);
      if (validation) throw new Error(validation);
      setRules(next);
      setSaved(false);
      setFormError(null);
    } catch (e) {
      setFormError(e instanceof Error ? e.message : t("Не удалось импортировать JSON."));
    } finally {
      if (fileInputRef.current) fileInputRef.current.value = "";
    }
  }

  const normalizedFilter = filter.trim().toLowerCase();
  const visibleRules = normalizedFilter ? rules.filter((rule) => `${rule.find}\n${rule.replace}`.toLowerCase().includes(normalizedFilter)) : rules;
  const activeCount = rules.filter((rule) => rule.enabled).length;
  const cleanRules = CLEAN_RULES();
  const activeCleanCount = cleanRules.filter((rule) => Boolean(formatting[rule.key])).length;
  const dictionarySize = formatting.custom_parasite_words.length + formatting.custom_words.length;
  const masterRule = MASTER_RULE();
  // The backend does not apply paused replacements in the full pass, while the
  // preview of that single stage always applies them — otherwise it would be
  // useless exactly when the rules are being set up. The discrepancy is
  // labelled.
  const matchesAreHypothetical = paused;
  // The diff is shown only when there is something to compare with; otherwise
  // the result card holds an "enter some text" prompt.
  const showPreviewDiff = Boolean(previewText.trim() && previewResult);

  return (
    <div className="page">
      <input type="file" accept=".json,application/json" ref={fileInputRef} style={{ display: "none" }} onChange={(e) => { const file = e.target.files?.[0]; if (file) void importRules(file); }}/>
      {/* There are no state pills in the header: both duplicated what is
          visible next to the blocks themselves — the rule counter sits on
          «Замены», and «не сохранено» lights up right there by the save button.
          A permanent green "saved" pill across the whole screen reported only
          that nothing had happened. */}
      <PageHeader title={t("Текст")}/>

      <div className="text-grid">
        <div className="flex-col" style={{ gap: 12, minWidth: 0 }}>
          <Foldable
            open={Boolean(folds.clean)}
            onToggle={() => toggleFold("clean")}
            title={t("Очистка")}
            summary={<span className="head-count">{activeCleanCount}/{cleanRules.length}</span>}
            /* There is no "enabled" pill next to the switch: it said exactly
               what the switch's position said, and in a narrow column it pushed
               the header onto a second line. */
            aside={<Hint text={masterRule.sub}><Switch on={formatting.enabled} onChange={(next) => void saveFormatting({ enabled: next })}/></Hint>}
          >
            <div className="fold__rows">
              {cleanRules.map((opt, i) => {
                const value = Boolean(formatting[opt.key]);
                return (
                  <div key={String(opt.key)} style={{ padding: "10px 12px", borderBottom: i === cleanRules.length - 1 ? "none" : "1px solid var(--line-soft)", display: "flex", alignItems: "center", gap: 10, opacity: formatting.enabled ? 1 : 0.55 }}>
                    <div className="flex-grow" style={{ minWidth: 0 }}>
                      <div style={{ font: "500 13px/1.2 var(--font-sans)", color: "var(--ink)" }}>{opt.title}</div>
                      <div style={{ font: "400 11.5px/1.4 var(--font-sans)", color: "var(--ink-mute)", marginTop: 2 }}>{opt.sub}</div>
                    </div>
                    <Switch on={value} onChange={(next) => void saveFormatting({ [opt.key]: next })}/>
                  </div>
                );
              })}
            </div>
          </Foldable>

          <Foldable
            open={Boolean(folds.repl)}
            onToggle={() => toggleFold("repl")}
            title={t("Замены")}
            summary={<>
              <span className="head-count">{activeCount}/{rules.length}</span>
              {!saved && <span className="pill warn">{t("не сохранено")}</span>}
            </>}
            aside={<Hint text={paused ? t("Замены на паузе") : t("Замены применяются")}><Switch on={!paused} onChange={(next) => void setPaused(!next)}/></Hint>}
          >
            <div style={{ padding: "10px 12px", display: "grid", gap: 10 }}>
              <div className="flex-row" style={{ gap: 8, flexWrap: "wrap" }}>
                <div className="input-search" style={{ flex: "1 1 180px" }}>
                  <Icon name="search" size={13} className="input-search__icon"/>
                  <input className="field" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={t("Найти правило")} style={{ height: 32 }}/>
                </div>
                <button className="btn btn--ghost" onClick={() => addRule()} disabled={savingRules}><Icon name="plus" size={13}/>{t("Добавить")}</button>
                <button className="btn btn--primary" onClick={() => void saveRules()} disabled={savingRules || saved}><Icon name="check" size={12}/>{savingRules ? t("Сохраняю") : t("Сохранить")}</button>
              </div>

              <div className="fold__rows">
                {visibleRules.length === 0 ? <ReplacementsEmptyState/> : <div>{visibleRules.map((rule, index) => <div key={rule.id} style={{ padding: 10, display: "grid", gap: 8, borderBottom: index < visibleRules.length - 1 ? "1px solid var(--line-soft)" : "none", opacity: rule.enabled ? 1 : 0.66 }}>
                  <div className="flex-row" style={{ gap: 8, flexWrap: "wrap" }}><Switch on={rule.enabled} onChange={(enabled) => updateRule(rule.id, { enabled })}/><span className="pill mono">{MATCH_LABELS()[rule.match]}</span><span className="pill mono">{rule.usage_count || 0}  {t("сраб.")}</span>{!rule.replace && <span className="pill warn">{t("удаляет текст")}</span>}<div style={{ marginLeft: "auto", display: "flex", gap: 4 }}><button className="btn btn--ghost" style={{ height: 24, padding: "0 7px" }} onClick={() => moveRule(rule.id, -1)} disabled={index === 0} aria-label={t("Поднять")}><Icon name="chev-down" size={12} style={{ transform: "rotate(180deg)" }}/></button><button className="btn btn--ghost" style={{ height: 24, padding: "0 7px" }} onClick={() => moveRule(rule.id, 1)} disabled={index === visibleRules.length - 1} aria-label={t("Опустить")}><Icon name="chev-down" size={12}/></button><button className="btn btn--ghost" style={{ height: 24, padding: "0 7px", color: "var(--err)" }} onClick={() => deleteRule(rule.id)} aria-label={t("Удалить")}><Icon name="trash" size={12}/></button></div></div>
                  <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) 24px minmax(0, 1fr)", gap: 8, alignItems: "center" }}><input ref={index === 0 ? findRef : undefined} className="field mono" value={rule.find} onChange={(e) => updateRule(rule.id, { find: e.target.value })} placeholder={t("что искать")} style={{ height: 30 }}/><Icon name="arrow-right" size={13} style={{ color: "var(--ink-mute)" }}/><input className="field mono" value={rule.replace} onChange={(e) => updateRule(rule.id, { replace: e.target.value })} placeholder={t("на что заменить")} style={{ height: 30 }}/></div>
                  <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(120px, 1fr))", gap: 8 }}><CustomSelect<string> value={rule.match} options={Object.entries(MATCH_LABELS()).map<SelectOption<string>>(([value, label]) => ({ value, label }))} onChange={(next) => updateRule(rule.id, { match: next as ReplacementMatchMode })}/><label className="pill" style={{ justifyContent: "center", cursor: "pointer" }}><input className="checkbox" type="checkbox" checked={rule.case_sensitive} onChange={(e) => updateRule(rule.id, { case_sensitive: e.target.checked })}/> Aa</label><label className="pill" style={{ justifyContent: "center", cursor: "pointer" }}><input className="checkbox" type="checkbox" checked={rule.preserve_case} onChange={(e) => updateRule(rule.id, { preserve_case: e.target.checked })}/>  {t("Регистр")}</label></div>
                </div>)}</div>}
                {formError && <div style={{ padding: "10px 12px", color: "var(--err)", font: "500 12px/1.4 var(--font-sans)", borderTop: "1px solid var(--line)" }}>{formError}</div>}
              </div>

              {/* Export and import are one-off operations, and on one line with
                  the search and «Сохранить» they stood between frequent actions.
                  The captions are mandatory: without them «Импорт» is just a
                  glyph. */}
              <div className="flex-row" style={{ gap: 6, justifyContent: "flex-end" }}>
                <button className="btn btn--ghost" style={{ height: 26 }} onClick={exportRules} disabled={rules.length === 0}><Icon name="download" size={12}/>{t("Экспорт")}</button>
                <button className="btn btn--ghost" style={{ height: 26 }} onClick={() => fileInputRef.current?.click()}><Icon name="folder" size={12}/>{t("Импорт")}</button>
              </div>
            </div>
          </Foldable>

          <Foldable
            open={Boolean(folds.dict)}
            onToggle={() => toggleFold("dict")}
            title={t("Словари")}
            summary={<span className="head-count">{dictionarySize} {tPlural(dictionarySize, ["слово", "слова", "слов"])}</span>}
          >
            <div style={{ padding: 14, display: "grid", gap: 14 }}>
              <div>
                <div className="flex-row" style={{ justifyContent: "space-between", marginBottom: 8 }}>
                  <div>
                    <div style={{ font: "600 13px/1.2 var(--font-sans)", color: "var(--ink)" }}>{t("Свои слова-паразиты")}</div>
                    <div style={{ font: "400 11.5px/1.4 var(--font-sans)", color: "var(--ink-mute)", marginTop: 2 }}>{t("По одному слову или фразе в строке. Также можно разделять запятыми.")}</div>
                  </div>
                  <button className="btn btn--ghost" onClick={saveCustomWords}><Icon name="check" size={12}/>{t("Сохранить")}</button>
                </div>
                <textarea className="field mono" value={customWordsText} onChange={(e) => setCustomWordsText(e.target.value)} onBlur={saveCustomWords} placeholder={t("например: собственно\nскажем так")} style={{ width: "100%", minHeight: 96, padding: 12, resize: "vertical", lineHeight: 1.45 }}/>
              </div>
              <div>
                <div className="flex-row" style={{ justifyContent: "space-between", marginBottom: 8 }}>
                  <div>
                    <div style={{ font: "600 13px/1.2 var(--font-sans)", color: "var(--ink)" }}>{t("Свой словарь")}</div>
                    <div style={{ font: "400 11.5px/1.4 var(--font-sans)", color: "var(--ink-mute)", marginTop: 2 }}>{t("Имена, бренды, термины и жаргон, которых движок знать не может. По одному в строке или через запятую.")}</div>
                  </div>
                  <button className="btn btn--ghost" onClick={saveDictionary}><Icon name="check" size={12}/>{t("Сохранить")}</button>
                </div>
                <textarea className="field mono" value={dictionaryText} onChange={(e) => setDictionaryText(e.target.value)} onBlur={saveDictionary} placeholder={t("например: Tauri\nClaude Code")} style={{ width: "100%", minHeight: 96, padding: 12, resize: "vertical", lineHeight: 1.45 }}/>
                {presets.length > 0 && (
                  <div className="flex-row" style={{ gap: 6, marginTop: 10, flexWrap: "wrap", alignItems: "center" }}>
                    <span style={{ font: "400 11px/1.5 var(--font-sans)", color: "var(--ink-mute)" }}>{t("Готовые наборы:")}</span>
                    {presets.map(([id, words]) => {
                      const on = (formatting.enabled_presets ?? []).includes(id);
                      return (
                        <Hint key={id} text={`${words.length} ${tPlural(words.length, ["термин", "термина", "терминов"])}`}><button className={on ? "btn btn--primary" : "btn btn--ghost"} type="button" style={{ height: 24, padding: "0 8px", font: "500 11px/1 var(--font-sans)" }} onClick={() => togglePreset(id)}>
                          <Icon name={on ? "check" : "plus"} size={11}/> {PRESET_LABELS()[id] ?? id}
                        </button>
                        </Hint>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>
          </Foldable>
        </div>

        {/* The heading and the explanation live inside the first card rather
            than above it: in the left column block headings sit inside the
            border, and an external caption above the card read as alien. The
            «до» and «после» steps are labelled with text — pills turned a
            utility caption into an accent that competed with the card's own
            content. */}
        <div className="flex-col" style={{ gap: 12, minWidth: 0 }}>
          <div className="card" style={{ padding: 18 }}>
            <div className="preview-card__head">
              <h2 className="preview-card__title">
                {t("Живой предпросмотр")}
                <Hint text={t("Весь локальный проход: очистка и замены — ровно то, что уходит в модель или во вставку.")}/>
              </h2>
              <span className="preview-card__step">{t("До")}</span>
            </div>
            <textarea className="field mono" value={previewText} onChange={(e) => setPreviewText(e.target.value)} placeholder={t("Введите текст для проверки обработки")} style={{ width: "100%", minHeight: 145, padding: 12, resize: "vertical", lineHeight: 1.55 }}/>
          </div>
          <div className="preview-pair__arrow preview-pair__arrow--down" aria-hidden="true"><Icon name="arrow-right" size={14}/></div>
          <div className="card" style={{ padding: 18 }}>
            <div className="preview-card__head">
              <span className="preview-card__step preview-card__step--after">{t("После")}</span>
            </div>
            {previewError ? <p style={{ margin: 0, font: "500 12px/1.55 var(--font-sans)", color: "var(--err)" }}>{previewError}</p> : <>
              {/* The result shows only the diff: it is the processed text,
                  simply with the changes highlighted. A separate paragraph above
                  it printed the same string a second time. */}
              {showPreviewDiff
                ? <DiffBlock before={previewText} after={previewResult} title={t("Diff: исходный → после обработки")} />
                : <p style={{ margin: 0, font: "400 13px/1.65 var(--font-sans)", color: "var(--ink-mute)", whiteSpace: "pre-wrap" }}>{t("Введите текст для предпросмотра")}</p>}
              <div className="flex-row" style={{ flexWrap: "wrap", gap: 6, marginTop: 10 }}>{previewMatches?.length ? previewMatches.map((item) => <span className={matchesAreHypothetical ? "pill" : "pill ok"} key={`${item.id}-${item.find}`}>{item.find}: {item.count}</span>) : <span className="pill">{t("Сработало 0 правил")} {rules.length === 0 ? t("— правил пока нет") : ""}</span>}</div>
              {matchesAreHypothetical && previewMatches?.length ? <div style={{ marginTop: 6, font: "400 11px/1.4 var(--font-sans)", color: "var(--warn)" }}>{t("Замены на паузе: правила совпали бы, но в результат выше не попали.")}</div> : null}
            </>}
          </div>
          <section className="card" style={{ padding: 14 }}>
            <div style={{ font: "600 13px/1.2 var(--font-sans)", color: "var(--ink)", display: "inline-flex", alignItems: "center", gap: 5, marginBottom: 8 }}>
              {t("Добавить правило-пример")}
              <Hint text={t("Нажмите, чтобы создать правило — оно сразу попадёт в список слева и в предпросмотр.")}/>
            </div>
            <div className="flex-row" style={{ flexWrap: "wrap", gap: 6 }}>{/* The ready-made rules are Russian words Whisper mishears. They do
                not go through t(): substituting an English word would create a
                rule that never fires. */}
              {/* i18n-ignore */}
              {[["щас", "сейчас"], ["тайпскрипт", "TypeScript"], ["мой мейл", "name@example.com"], ["смайл", ":)"]].map(([find, replace]) => <button key={find} className="btn btn--ghost" onClick={() => addRule(find, replace)} style={{ height: 26 }}><span className="mono">{find}</span><Icon name="arrow-right" size={11}/><span className="mono">{replace}</span></button>)}</div>
          </section>
        </div>
      </div>
    </div>
  );
}

function hotkeyParts(hotkey?: string): string[] {
  const labels: Record<string, string> = { ctrl: "Ctrl", control: "Ctrl", shift: "Shift", alt: "Alt", win: "Win", cmd: "Win", super: "Win", space: "Space", enter: "Enter", esc: "Esc", tab: "Tab" };
  return (hotkey || DEFAULT_HOTKEY).split("+").map((part) => labels[part.trim().toLowerCase()] ?? part.trim().toUpperCase()).filter(Boolean);
}

function KbdSequence({ keys }: { keys: string[] }) {
  return <span style={{ display: "inline-flex", alignItems: "center", gap: 4, flexWrap: "wrap" }}>{keys.map((key, i) => <span key={`${key}-${i}`} style={{ display: "inline-flex", alignItems: "center", gap: 4 }}><span className="kbd">{key}</span>{i < keys.length - 1 && <span style={{ color: "var(--text-mute)" }}>+</span>}</span>)}</span>;
}

function HelpCard({ title, icon, children, accent = false }: { title: string; icon?: string; children: ReactNode; accent?: boolean }) {
  return (
    <section className={`card${accent ? " accent" : ""}`} style={{ padding: 20, minWidth: 0 }}>
      <h2 style={{ margin: "0 0 14px", display: "flex", alignItems: "center", gap: 8, font: "600 14px/1.2 var(--font-sans)", color: "var(--ink)" }}>{icon && <span style={{ color: "var(--accent-text)", display: "flex" }}><Icon name={icon} size={15}/></span>}{title}</h2>
      {children}
    </section>
  );
}

function InfoRow({ label, value }: { label: string; value: ReactNode }) {
  return <div style={{ display: "flex", justifyContent: "space-between", gap: 12, padding: "8px 0", borderBottom: "1px solid var(--line-soft)", font: "500 12px/1.25 var(--font-sans)" }}><span style={{ color: "var(--ink-mute)" }}>{label}</span><span style={{ color: "var(--ink)", textAlign: "right" }}>{value}</span></div>;
}

function PipelineStep({ index, title, detail, icon }: { index: number; title: string; detail: string; icon: string }) {
  return (
    <div style={{ padding: 14, borderRadius: "var(--radius)", background: "var(--bg-2)", border: "1px solid var(--line)" }}>
      <div className="flex-row" style={{ gap: 8, marginBottom: 8 }}>
        <span style={{ font: "500 10px/1 var(--font-mono)", color: "var(--ink-mute)", background: "var(--bg-4)", padding: "2px 6px", borderRadius: 4 }}>{index}</span>
        <span className="card-icon" style={{ width: 26, height: 26, color: "var(--accent-text)" }}><Icon name={icon} size={13}/></span>
        <span style={{ font: "600 13px/1.2 var(--font-sans)", color: "var(--ink)" }}>{title}</span>
      </div>
      <div style={{ font: "400 12px/1.5 var(--font-sans)", color: "var(--ink-mute)" }}>{detail}</div>
    </div>
  );
}

type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "available"; info: UpdateInfo }
  | { kind: "downloading"; info: UpdateInfo; progress: UpdateDownloadProgress | null }
  | { kind: "error"; message: string };

function formatMb(bytes: number) {
  return t("{p0} МБ", { p0: (bytes / 1024 / 1024).toFixed(1) });
}

// An update is never installed by itself: the check when the page opens is
// silent (there is nothing to gain from showing a network error) and downloading
// happens only on an explicit click. The user always sees exactly what is
// coming: the version, the date and the release notes.
function UpdatesCard({ version }: { version?: string | null }) {
  const [state, setState] = useState<UpdateState>({ kind: "idle" });

  async function check(loud: boolean) {
    setState({ kind: "checking" });
    try {
      const info = await invoke<UpdateInfo>("check_update");
      setState(info.available ? { kind: "available", info } : { kind: "current" });
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      // A silent check on open must not shout about a missing network.
      setState(loud ? { kind: "error", message } : { kind: "idle" });
    }
  }

  useEffect(() => { void check(false); }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void on<UpdateDownloadProgress>("update-download-progress", (progress) => {
      setState((current) => current.kind === "downloading" ? { ...current, progress } : current);
    }).then((fn) => { unlisten = fn; });
    return () => unlisten?.();
  }, []);

  async function install(info: UpdateInfo) {
    setState({ kind: "downloading", info, progress: null });
    try {
      // On success the application restarts and we never return here.
      await invoke("install_update");
    } catch (e) {
      setState({ kind: "error", message: e instanceof Error ? e.message : String(e) });
    }
  }

  const busy = state.kind === "checking" || state.kind === "downloading";
  const percent = state.kind === "downloading" && state.progress?.total
    ? Math.round((state.progress.downloaded / state.progress.total) * 100)
    : null;

  return (
    <HelpCard title={t("Обновления")} icon="download">
      <div style={{ display: "grid", gap: 0 }}>
        <InfoRow label={t("Установленная версия")} value={<span className="mono">{version ?? "0.0.0"}</span>}/>
        {state.kind === "available" && <InfoRow label={t("Доступна версия")} value={<span className="mono" style={{ color: "var(--accent-text)" }}>{state.info.version}</span>}/>}
        {state.kind === "available" && state.info.date && <InfoRow label={t("Опубликована")} value={state.info.date.slice(0, 10)}/>}
      </div>

      {state.kind === "available" && state.info.notes && (
        <div style={{ marginTop: 12 }}>
          <SectionLabel>{t("Что нового")}</SectionLabel>
          <div style={{ maxHeight: 180, overflow: "auto", padding: "10px 12px", borderRadius: "var(--r-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 12px/1.55 var(--font-sans)", color: "var(--ink-dim)", whiteSpace: "pre-wrap" }}>
            {state.info.notes}
          </div>
        </div>
      )}

      {state.kind === "downloading" && (
        <div style={{ marginTop: 12 }}>
          <div style={{ position: "relative", height: 4, borderRadius: 999, background: "var(--surface-3)", overflow: "hidden" }}>
            {percent != null
              ? <div style={{ position: "absolute", inset: 0, width: `${percent}%`, background: "var(--accent)", borderRadius: 999, transition: "width 200ms ease" }}/>
              : <div style={{ position: "absolute", inset: 0, width: "40%", background: "linear-gradient(90deg, transparent, var(--accent), transparent)", animation: "progress-sweep 1.15s ease-in-out infinite", borderRadius: 999 }}/>}
          </div>
          <p style={{ margin: "8px 0 0", font: "400 11.5px/1.4 var(--font-sans)", color: "var(--ink-mute)" }}>
            {state.progress
              ? t("Скачано {p0}{p1}. Приложение перезапустится само.", { p0: formatMb(state.progress.downloaded), p1: state.progress.total ? ` из ${formatMb(state.progress.total)}` : "" })
              : t("Скачиваем обновление. Приложение перезапустится само.")}
          </p>
        </div>
      )}

      {state.kind === "current" && <p style={{ margin: "12px 0 0", font: "400 11.5px/1.4 var(--font-sans)", color: "var(--ink-mute)" }}>{t("Установлена последняя версия.")}</p>}
      {state.kind === "error" && <p style={{ margin: "12px 0 0", font: "400 11.5px/1.4 var(--font-sans)", color: "var(--err)" }}>{state.message}</p>}

      <div className="flex-row" style={{ gap: 8, marginTop: 12, flexWrap: "wrap" }}>
        <button className="btn btn--ghost" type="button" disabled={busy} onClick={() => void check(true)}>
          <Icon name="refresh" size={13}/> {state.kind === "checking" ? t("Проверяем…") : t("Проверить обновления")}
        </button>
        {state.kind === "available" && (
          <button className="btn btn--primary" type="button" onClick={() => void install(state.info)}>
            <Icon name="download" size={13}/>  {t("Обновить до")} {state.info.version}
          </button>
        )}
      </div>
    </HelpCard>
  );
}

// A set's name lives here rather than in Rust: the word list is the same for
// every language, while the button's caption is translated.
const PRESET_LABELS = (): Record<string, string> => ({
  development: t("Разработка"),
});

const LOG_LEVELS = ["error", "warn", "info", "debug", "trace"] as const;

// The logs rotate at 5 MB and keep three archives, so the size lives between
// kilobytes and a couple of dozen megabytes. "0.0 MB" on a fresh install reports
// nothing, so small values are shown in kilobytes.
function formatLogSize(bytes: number) {
  return bytes < 1024 * 1024
    ? t("{p0} КБ", { p0: Math.round(bytes / 1024).toString() })
    : t("{p0} МБ", { p0: (bytes / 1024 / 1024).toFixed(1) });
}

// Diagnosing somebody else's problem rests on what they send you. Here are the
// three things they can send: the log level, an environment summary, and the
// saved recordings.
function DiagnosticsCard({ config, onConfigChanged }: { config: ConfigResult | null; onConfigChanged?: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null> }) {
  const [report, setReport] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [logsBytes, setLogsBytes] = useState<number | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const logLevel = config?.log_level ?? "info";
  const saveRecordings = config?.debug_save_recordings ?? false;
  const overlayDiag = config?.debug_overlay_diag ?? false;

  // The size is read when the page opens and after a cleanup. It changes by a
  // megabyte a week; there is no point watching it in real time.
  useEffect(() => {
    void invoke<number>("logs_size").then(setLogsBytes).catch(() => setLogsBytes(null));
  }, []);

  // Clearing takes two clicks: the logs are the only trace of a problem that
  // has already happened, and a mis-click erases exactly what brought the person
  // to this page.
  async function clearLogs() {
    if (!confirmClear) {
      setConfirmClear(true);
      window.setTimeout(() => setConfirmClear(false), 4000);
      return;
    }
    setConfirmClear(false);
    try {
      setLogsBytes(await invoke<number>("clear_logs"));
    } catch {
      // An auxiliary action: there is nothing here worth failing over.
    }
  }

  async function copyReport() {
    try {
      const text = await invoke<string>("get_diagnostics");
      setReport(text);
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      setReport(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <HelpCard title={t("Диагностика")} icon="info">
      <div style={{ display: "grid", gap: 0 }}>
        <InfoRow
          label={t("Подробность логов")}
          value={
            <div style={{ width: 150 }}>
              <CustomSelect<string>
                value={logLevel}
                disabled={!onConfigChanged}
                options={LOG_LEVELS.map<SelectOption<string>>((level) => ({ value: level, label: level }))}
                onChange={(next) => void onConfigChanged?.({ log_level: next as ConfigResult["log_level"] })}
              />
            </div>
          }
        />
        <InfoRow
          label={t("Размер логов")}
          value={
            <span style={{ color: "var(--ink-mute)", font: "500 11.5px/1 var(--font-mono)" }}>
              {logsBytes === null ? "—" : formatLogSize(logsBytes)}
            </span>
          }
        />
        <InfoRow
          label={t("Сохранять записи в WAV")}
          value={
            <Hint text={t("Класть каждую запись рядом с логами. Нужно, чтобы воспроизвести жалобу «распознало не то».")}>
            <label className="checkbox-row" style={{ justifyContent: "flex-end" }}>
              <input className="checkbox" type="checkbox" checked={saveRecordings} disabled={!onConfigChanged} onChange={(e) => void onConfigChanged?.({ debug_save_recordings: e.target.checked })}/>
              <span style={{ color: "var(--ink-mute)", font: "500 11.5px/1 var(--font-sans)" }}>{saveRecordings ? t("включено") : t("выключено")}</span>
            </label>
            </Hint>
          }
        />
        <InfoRow
          label={t("Диагностика оверлея")}
          value={
            <Hint text={t("Писать в лог стили и список окон на каждом показе и скрытии оверлея. Нужно только для разбора мигающей системной рамки; в логи попадают заголовки открытых окон.")}>
            <label className="checkbox-row" style={{ justifyContent: "flex-end" }}>
              <input className="checkbox" type="checkbox" checked={overlayDiag} disabled={!onConfigChanged} onChange={(e) => void onConfigChanged?.({ debug_overlay_diag: e.target.checked })}/>
              <span style={{ color: "var(--ink-mute)", font: "500 11.5px/1 var(--font-sans)" }}>{overlayDiag ? t("включено") : t("выключено")}</span>
            </label>
            </Hint>
          }
        />
      </div>
      <div className="flex-row" style={{ gap: 8, marginTop: 12, flexWrap: "wrap" }}>
        <button className="btn btn--ghost" type="button" onClick={() => void invoke("open_diagnostics_folder").catch(() => {})}>
          <Icon name="folder" size={12}/>  {t("Открыть папку логов")} </button>
        <button className="btn btn--ghost" type="button" onClick={() => void copyReport()}>
          <Icon name="copy" size={12}/> {copied ? t("Скопировано") : t("Скопировать сводку")}
        </button>
        <button className="btn btn--ghost" type="button" onClick={() => void clearLogs()}>
          <Icon name="trash" size={12}/> {confirmClear ? t("Точно очистить?") : t("Очистить логи")}
        </button>
      </div>
      {report && (
        <pre style={{ margin: "12px 0 0", padding: 10, background: "var(--bg-2)", border: "1px solid var(--line)", borderRadius: "var(--radius)", font: "500 11px/1.5 var(--font-mono)", color: "var(--ink-mute)", whiteSpace: "pre-wrap", overflowX: "auto" }}>{report}</pre>
      )}
      {saveRecordings && (
        <p style={{ margin: "10px 0 0", font: "400 11.5px/1.5 var(--font-sans)", color: "var(--ink-mute)" }}>
           {t("Записи с микрофона пишутся на диск. Хранятся последние 50, старые удаляются автоматически.")} </p>
      )}
    </HelpCard>
  );
}

export function InfoPage({ version, config, onConfigChanged }: { version?: string | null; config: ConfigResult | null; onConfigChanged?: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null> }) {
  const pipelineMode = config?.ai_processing?.pipeline_mode ?? "local";
  const hotkey = hotkeyParts(config?.hotkey);
  const recordingMode = config?.recording_mode === "push_to_talk" ? t("Удержание клавиш") : t("Переключатель");
  const llmThreshold = config?.ai_processing?.llm_min_duration_seconds ?? 0;

  const pipelineSteps = [
    { icon: "kbd", title: t("Горячая клавиша"), detail: t("Приложение слушает глобальный hotkey, не забирая фокус у текущего окна. При старте запоминается окно, куда потом нужно вставить текст.") },
    { icon: "mic", title: t("Запись и overlay"), detail: t("Микрофон пишет фразу, поверх экрана появляется компактный overlay с уровнем звука и кнопкой отмены.") },
    { icon: "cpu", title: t("Локальное распознавание"), detail: t("Аудио распознается локальной моделью. На этом этапе появляется сырой текст без ручной правки.") },
    { icon: "wand", title: t("Форматирование"), detail: t("Локальные правила удаляют заполнители, слова-паразиты, повторы, лишние пробелы и добавляют базовую пунктуацию.") },
    { icon: "spark", title: t("LLM-этап"), detail: pipelineMode === "local" ? t("В текущем режиме LLM пропускается: итогом становится локально отформатированный текст.") : llmThreshold > 0 ? t("Если запись не короче {p0} с, текст отправляется выбранному LLM-провайдеру. При таймауте вставляется локальный fallback.", { p0: llmThreshold }) : t("Текст отправляется выбранному LLM-провайдеру. При таймауте вставляется локальный fallback.") },
    { icon: "copy", title: t("Вставка и история"), detail: t("Готовый текст вставляется в исходное окно, а запись сохраняется в истории вместе с raw/formatted/final версиями и статистикой обработки.") },
  ];

  return (
    <div className="page">
      <PageHeader
        title={t("Справка")}
      />

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.15fr) minmax(280px, .85fr)", gap: 14, marginBottom: 14 }} className="help-top">
        <HelpCard title={t("Как пользоваться")} icon="play" accent>
          <div style={{ display: "grid", gap: 10 }}>
            {[t("Откройте окно, куда нужно вставить результат."), t("Нажмите горячую клавишу и продиктуйте фразу."), t("Остановите запись тем же hotkey или отпустите клавиши в режиме удержания."), t("Дождитесь обработки: текст вставится автоматически, если включен auto-paste.")].map((text, i) => <div key={text} style={{ display: "grid", gridTemplateColumns: "22px 1fr", gap: 14, alignItems: "start", padding: "10px 0", borderBottom: i < 3 ? "1px solid var(--line-soft)" : "none" }}><span style={{ width: 22, height: 22, borderRadius: "50%", background: "var(--accent-soft)", color: "var(--accent-text)", display: "grid", placeItems: "center", font: "600 11px/1 var(--font-mono)" }}>{i + 1}</span><span style={{ font: "500 13.5px/1.55 var(--font-sans)", color: "var(--ink)" }}>{text}</span></div>)}
          </div>
        </HelpCard>

        <HelpCard title={t("Текущие команды")} icon="kbd">
          <div style={{ display: "grid", gap: 0 }}>
            <InfoRow label={t("Начать / остановить")} value={<KbdSequence keys={hotkey}/>}/>
            <InfoRow label={t("Отменить overlay")} value={<KbdSequence keys={["Esc"]}/>}/>
            <InfoRow label={t("Режим записи")} value={recordingMode}/>
            <InfoRow label={t("Автовставка")} value={config?.auto_paste ? t("Включена") : t("Выключена")}/>
          </div>
          {/* The "in … mode recording runs …" line is gone: the mode itself is
              on the row above, and how it works is written in the hint on
              «Режим записи» in settings, in the same place it is switched. */}
        </HelpCard>
      </div>

      <HelpCard title={t("Pipeline записи")} icon="spark">
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(250px, 1fr))", gap: 12 }}>
          {pipelineSteps.map((step, i) => <PipelineStep key={step.title} index={i + 1} title={step.title} detail={step.detail} icon={step.icon}/>) }
        </div>
      </HelpCard>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) minmax(280px, .85fr)", gap: 14, marginTop: 14 }} className="help-top">
        <DiagnosticsCard config={config} onConfigChanged={onConfigChanged}/>
        <UpdatesCard version={version}/>
      </div>

      <style>{`
        @media (max-width: 1100px) { .help-top { grid-template-columns: 1fr !important; } }
      `}</style>
    </div>
  );
}
