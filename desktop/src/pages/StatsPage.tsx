import { useId, useState } from "react";
import type { StatsResult } from "../bridge/types";
import { PageHeader, Segmented } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Hint } from "../components/Hint";
import { localeTag, t, tPlural } from "../i18n";
import { timingChartPath } from "./timingChart";

type StatsRange = "week" | "month" | "year" | "all";
type DailyStats = {
  date: string;
  count: number;
  chars: number;
  time_saved_seconds: number;
  audio_seconds: number;
  excluded_silence_seconds: number;
  speech_timed_count: number;
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
  return { date, count: 0, chars: 0, time_saved_seconds: 0, audio_seconds: 0, excluded_silence_seconds: 0, speech_timed_count: 0, processing_seconds: 0, whisper_seconds: 0, format_seconds: 0, llm_seconds: 0, llm_attempts: 0, llm_used: 0, llm_fallbacks: 0, llm_input_tokens: 0, llm_output_tokens: 0, llm_tokens: 0 };
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
  const prefix = seconds < 0 && Math.round(Math.abs(seconds) / 60) > 0 ? "-" : "";
  return `${prefix}${formatDuration(Math.abs(seconds))}`;
}

/** Explain the savings estimate by how many recordings had their pauses measured. */
function savingsText(timedCount: number, transcriptions: number, excludedSilenceSeconds: number): { sub: string; hint: string } {
  if (timedCount === 0) {
    return {
      sub: t("минус аудио и обработка"),
      hint: t("Оценка ручного набора минус полная длительность аудио и обработка. Замеров речи за этот период нет."),
    };
  }
  const method = `${t("Оценка ручного набора минус время речи с короткими паузами и обработка. Длинные паузы исключаются с запасом у границ фраз.")} ${t("Из аудио исключено: {p0}.", { p0: formatShortDuration(excludedSilenceSeconds) })}`;
  if (transcriptions > 0 && timedCount === transcriptions) {
    return { sub: t("без длительных пауз"), hint: method };
  }
  return {
    sub: t("паузы учтены частично"),
    hint: `${t("Паузы оценены в {p0} из {p1} записей. Остальные учтены по полной длительности аудио.", { p0: timedCount, p1: transcriptions })} ${method}`,
  };
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
      excluded_silence_seconds: Number(item.excluded_silence_seconds) || 0,
      speech_timed_count: Number(item.speech_timed_count) || 0,
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
  const colors = ["var(--bg-2)", ...[18, 40, 65].map((share) => `color-mix(in srgb, var(--accent) ${share}%, transparent)`), "var(--accent)"];
  const cells = buildDailySeries(history, "year");
  const max = Math.max(1, ...cells.map((item) => item.count));
  const monthLabels = [0, 4, 8].map((week) => {
    const cell = cells[Math.min(cells.length - 1, week * 7)];
    return new Date(`${cell.date}T00:00:00`).toLocaleDateString(localeTag(), { month: "short" }).replace(".", "");
  });
  return <div><div style={{ display: "flex", justifyContent: "space-around", marginBottom: 8, paddingLeft: 28 }}>{monthLabels.map((m, i) => <span key={`${m}-${i}`} style={{ font: "500 10px/1 var(--font-mono)", color: "var(--ink-mute)" }}>{m}</span>)}</div><div style={{ display: "flex", gap: 8 }}><div style={{ display: "flex", flexDirection: "column", justifyContent: "space-between", font: "500 10px/1 var(--font-mono)", color: "var(--ink-mute)", paddingTop: 1, paddingBottom: 1 }}>{[t("Пн"), "", t("Ср"), "", t("Пт"), "", ""].map((d, i) => <span key={i} style={{ height: 12 }}>{d}</span>)}</div><div style={{ display: "grid", gridTemplateColumns: "repeat(12, minmax(10px, 1fr))", gap: 3, flex: 1 }}>{Array.from({ length: 12 }).map((_, w) => <div key={w} className="heatmap-week" style={{ display: "grid", gridTemplateRows: "repeat(7, 1fr)", gap: 3 }}>{Array.from({ length: 7 }).map((_, d) => { const item = cells[w * 7 + d]; const level = item.count === 0 ? 0 : Math.max(1, Math.ceil((item.count / max) * 4)); return <div key={item.date} className="heatmap-cell" style={{ width: "100%", aspectRatio: 1, borderRadius: 3, background: colors[level] }}><div className="heatmap-popover" role="tooltip"><div className="heatmap-popover__title">{longDateLabel(item.date)}</div><div>{item.count.toLocaleString(localeTag())}  {t("распознаваний")}</div><div>{item.chars.toLocaleString(localeTag())}  {t("символов")}</div><div>{t("Аудио:")} {formatShortDuration(item.audio_seconds)}</div><div>{t("Обработка:")} {formatShortDuration(item.processing_seconds)}</div>{item.llm_attempts > 0 && <div>LLM: {item.llm_used}/{item.llm_attempts}  {t("успешно")}</div>}{item.llm_tokens > 0 && <div>{t("Токены:")} {item.llm_tokens.toLocaleString(localeTag())}</div>}</div></div>; })}</div>)}</div></div></div>;
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
  const gradientId = useId();
  const maxY = Math.max(0.01, ...data.flatMap((d) => [d.whisper_seconds, d.llm_seconds]));
  const whisperPath = timingChartPath(data.map((d) => d.whisper_seconds), maxY);
  const llmPath = timingChartPath(data.map((d) => d.llm_seconds), maxY);
  return (
    <svg role="img" aria-label={t("Время этапа по дням")} viewBox="0 0 100 100" preserveAspectRatio="none" style={{ width: "100%", height: 200, overflow: "visible" }}>
      <defs>
        <linearGradient id={`${gradientId}-stt`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="var(--accent)" stopOpacity="0.18"/>
          <stop offset="100%" stopColor="var(--accent)" stopOpacity="0"/>
        </linearGradient>
        <linearGradient id={`${gradientId}-llm`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="var(--info)" stopOpacity="0.12"/>
          <stop offset="100%" stopColor="var(--info)" stopOpacity="0"/>
        </linearGradient>
      </defs>
      {[0, 25, 50, 75, 100].map((y) => (
        <line key={y} x1="0" y1={y} x2="100" y2={y} stroke="var(--line-soft)" strokeWidth="0.6" vectorEffect="non-scaling-stroke"/>
      ))}
      {llmPath && <path d={`${llmPath} L 100 100 L 0 100 Z`} fill={`url(#${gradientId}-llm)`}/>}
      {whisperPath && <path d={`${whisperPath} L 100 100 L 0 100 Z`} fill={`url(#${gradientId}-stt)`}/>}
      <path d={llmPath} fill="none" stroke="var(--info)" strokeWidth="2" vectorEffect="non-scaling-stroke" strokeLinecap="round" strokeLinejoin="round"/>
      <path d={whisperPath} fill="none" stroke="var(--accent)" strokeWidth="2" vectorEffect="non-scaling-stroke" strokeLinecap="round" strokeLinejoin="round"/>
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
        return <Hint key={d.date} asChild text={`${shortDateLabel(d.date)}: ${d.count}`}><div className={`bar-row__cell${d.count > 0 ? " active" : ""}`} style={{ height: `${height}%` }}/></Hint>;
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
  const excludedSilenceSeconds = Math.max(0, Math.min(audioSeconds, pick(stats?.total_excluded_silence_seconds, "excluded_silence_seconds")));
  const speechTimedCount = pick(stats?.total_speech_timed_transcriptions, "speech_timed_count");
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
  const netSavedSeconds = manualTypingSeconds - (audioSeconds - excludedSilenceSeconds) - processingSeconds;
  const savings = savingsText(speechTimedCount, transcriptions, excludedSilenceSeconds);
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
          <Hint asChild text={t("Скорость ручного набора из настроек")}><span className="head-count">{speedCpm.toLocaleString(localeTag())}  {t("симв/мин")}</span></Hint>
          <Segmented value={range} options={RANGE_OPTIONS()} onChange={(value) => setRange(value as StatsRange)}/>
          <button className="btn btn--ghost" onClick={() => void refresh()} disabled={refreshing}><Icon name="refresh" size={13}/>{refreshing ? t("Обновляю") : t("Обновить")}</button>
        </>}
      />

      <div className="stats-grid">
        <Stat label={t("Распознаваний")} value={transcriptions.toLocaleString(localeTag())} sub={periodSub}/>
        <Stat label={t("Символов")} value={chars.toLocaleString(localeTag())} sub={t("в среднем {p0} на запись", { p0: averageChars.toLocaleString(localeTag()) })}/>
        <Stat label={t("Ручной набор")} value={formatDuration(manualTypingSeconds)} sub={t("Символы / {p0} симв/мин.", { p0: speedCpm.toLocaleString(localeTag()) })}/>
        <Stat label={t("Оценка экономии")} value={formatSignedDuration(netSavedSeconds)} sub={savings.sub} accent={netSavedSeconds >= 0} hint={savings.hint}/>
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
