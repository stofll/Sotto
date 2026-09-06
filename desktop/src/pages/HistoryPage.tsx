import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke, on } from "../bridge";
import {
    applyHistoryAiProcessing,
    clearHistory,
    deleteHistoryEntry,
    listHistory,
    previewHistoryAiProcessing,
} from "../bridge/stats";
import { Card, PageHeader, Segmented } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Hint } from "../components/Hint";
import { confirmDestructive } from "../components/ConfirmDialog";
import { CustomSelect } from "../components/CustomSelect";
import { DiffBlock } from "../components/DiffBlock";
import type { ConfigResult, HistoryAiPreview, HistoryEntry } from "../bridge/types";
import { effectiveSystemPrompt } from "./aiShared";
import { localeTag, t, tPlural } from "../i18n";

type AiConfig = ConfigResult["ai_processing"];
type AiProfile = NonNullable<AiConfig["profiles"]>[number];
type ViewMode = "cards" | "list";
type StatusFilter = "all" | "processed" | "fallback" | "skipped";
type DateFilter = "all" | "today" | "week";

function formatTime(unix: number): string {
  const date = new Date(unix * 1000);
  const now = new Date();
  const sameDay = date.toDateString() === now.toDateString();
  const time = date.toLocaleTimeString(localeTag(), { hour: "2-digit", minute: "2-digit" });
  if (sameDay) return time;
  return `${date.toLocaleDateString(localeTag(), { day: "2-digit", month: "short" })} · ${time}`;
}

function aiProfileLabel(entry: HistoryEntry): string | null {
  const ai = entry.ai_processing;
  if (!ai) return null;
  if (ai.profile_name) return ai.profile_name;
  const parts = [ai.provider, ai.model].filter(Boolean);
  return parts.length ? parts.join(" / ") : null;
}

function transcriptionModelLabel(entry: HistoryEntry): string {
  const model = entry.transcription_model?.trim();
  return model || t("модель не сохранена");
}

function relativeAge(unix: number): string {
  const seconds = Math.max(0, Math.floor(Date.now() / 1000 - unix));
  if (seconds < 60) return t("только что");
  if (seconds < 3600) return t("{p0} мин назад", { p0: Math.floor(seconds / 60) });
  if (seconds < 24 * 3600) return t("{p0} ч назад", { p0: Math.floor(seconds / 3600) });
  return t("{p0} д назад", { p0: Math.floor(seconds / 86400) });
}

function dayBucketLabel(unix: number): string {
  const date = new Date(unix * 1000);
  const now = new Date();
  const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const diffDays = Math.round((startOfDay(now) - startOfDay(date)) / 86_400_000);
  if (diffDays === 0) return t("Сегодня");
  if (diffDays === 1) return t("Вчера");
  if (diffDays < 7) return date.toLocaleDateString(localeTag(), { weekday: "long" });
  return date.toLocaleDateString(localeTag(), { day: "2-digit", month: "long", year: date.getFullYear() === now.getFullYear() ? undefined : "numeric" });
}

function aiStatusKind(entry: HistoryEntry): "processed" | "fallback" | "skipped" | "none" {
  const ai = entry.ai_processing;
  if (!ai || Object.keys(ai).length === 0) return "none";
  if (ai.attempted && ai.used) return "processed";
  if (ai.attempted && ai.fallback) return "fallback";
  return "skipped";
}

function aiStatusText(entry: HistoryEntry): string {
  const ai = entry.ai_processing;
  if (!ai || Object.keys(ai).length === 0) return t("LLM: нет данных");
  if (!ai.enabled) return t("LLM: выключено");
  const profile = ai.profile_name ? `${ai.profile_name} · ` : "";
  const model = `${profile}${[ai.provider, ai.model].filter(Boolean).join(" / ")}`.trim();
  if (ai.attempted && ai.used) return model ? t("LLM: обработано · {p0}", { p0: model }) : t("LLM: обработано");
  if (ai.attempted && ai.fallback) {
    const label = aiFallbackLabel(ai.error_type, ai.skipped_reason);
    return model ? `LLM: ${label} · ${model}` : `LLM: ${label}`;
  }
  if (ai.skipped_reason === "duration_below_threshold") return t("LLM: пропущено · короче {p0} сек", { p0: Math.round(ai.min_duration_seconds ?? 0) });
  if (ai.skipped_reason === "missing_api_key") return t("LLM: пропущено · нет ключа");
  if (ai.skipped_reason === "missing_provider") return t("LLM: пропущено · нет провайдера");
  return t("LLM: пропущено");
}

function aiFallbackLabel(errorType?: string, skippedReason?: string): string {
  const code = errorType || skippedReason || "";
  if (code === "auth_error" || code === "provider_auth_error") return t("ошибка ключа");
  if (code === "rate_limit" || code === "provider_quota_or_rate_limit") return t("лимит");
  if (code === "timeout" || code === "provider_timeout") return "timeout";
  if (code === "connection_error" || code === "provider_connection_error") return t("сеть");
  if (code === "bad_response" || code === "provider_bad_response") return t("неожиданный ответ");
  if (code === "empty_response") return t("пустой ответ");
  if (code === "meta_response" || code === "model_returned_meta_response") return "meta fallback";
  if (code === "summarised_response" || code === "model_dropped_text") return t("модель сократила текст");
  return "fallback";
}

// Rust returns the raw `skipped_reason` code rather than a sentence, so the
// wording for a given failure lives in exactly one place. The provider-side
// codes are already spelled out by `aiFallbackLabel`; only the gates that
// stop the call before it leaves the app need their own text.
function aiSkipLabel(code: string): string {
  if (code === "local_mode") return t("режим «локально» — LLM выключена");
  if (code === "missing_provider") return t("не выбран провайдер");
  if (code === "missing_api_key") return t("нет ключа");
  if (code === "duration_below_threshold") return t("запись короче порога");
  const label = aiFallbackLabel(undefined, code);
  // An unmapped code is more useful raw than as the word "fallback".
  return label === "fallback" ? code : label;
}

function aiStatusColor(entry: HistoryEntry): string {
  const ai = entry.ai_processing;
  if (ai?.attempted && ai.used) return "var(--ok)";
  if (ai?.attempted && ai.fallback) return "var(--err)";
  return "var(--ink-mute)";
}

function aiTargetText(ai: Pick<AiConfig, "provider" | "model"> | HistoryEntry["ai_processing"] | null | undefined): string {
  const provider = ai?.provider?.trim();
  const model = ai?.model?.trim();
  return [provider, model].filter(Boolean).join(" / ") || t("провайдер не выбран");
}

function formatSeconds(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "-";
  if (value < 0.1) return t("{p0} мс", { p0: Math.round(value * 1000) });
  return t("{p0} с", { p0: value.toFixed(value < 10 ? 1 : 0) });
}

function processingStatsText(entry: HistoryEntry): string {
  const stats = entry.processing_stats;
  const audioSeconds = stats?.audio_seconds ?? entry.ai_processing?.audio_duration_seconds;
  const parts = [
    [t("Аудио"), audioSeconds],
    ["STT", stats?.whisper_seconds],
    [t("Формат"), stats?.format_seconds],
    ["LLM", stats?.llm_seconds],
    [t("Всего"), stats?.total_seconds],
  ] as const;
  const timing = parts
    .filter(([, value]) => typeof value === "number" && Number.isFinite(value))
    .map(([label, value]) => `${label} ${formatSeconds(value)}`)
    .join(" · ");
  const replacements = stats?.replacement_stats?.total;
  return typeof replacements === "number" && replacements > 0 ? t("{p0} · Замен {p1}", { p0: timing, p1: replacements }) : timing;
}

/** The prompt the chosen profile runs on, resolved here because the presets
 *  are the frontend's: a profile that never edited its own carries an empty
 *  `system_prompt` and means «my `prompt_preset`» by it. Rust inherits the
 *  dictation prompt for an empty one, which is the right answer only when no
 *  profile was chosen at all. */
export function reprocessPrompt(aiConfig: AiConfig | null, profileId: string): string | undefined {
  const profile = (aiConfig?.profiles ?? []).find((item) => item.id === profileId);
  return profile ? effectiveSystemPrompt(profile) : undefined;
}

/** The text a manual run is fed: the local-processing result if there is one,
 *  otherwise whatever the row currently shows. Same fallback as the Rust side.
 *  A successful earlier run is not a reason to refuse — «прогнать другим
 *  профилем» is the most common thing to want from a processed entry. */
function reprocessSource(entry: HistoryEntry): string {
  return (entry.formatted_text || entry.text || "").trim();
}

async function copyToClipboard(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {/* fall through */}
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    return ok;
  } catch {
    return false;
  }
}

function entryHasDetails(entry: HistoryEntry): boolean {
  if (entry.formatted_text && entry.formatted_text !== entry.text) return true;
  if (entry.raw_text && entry.raw_text !== entry.formatted_text && entry.raw_text !== entry.text) return true;
  if (entry.ai_processing?.provider_error) return true;
  if (processingStatsText(entry)) return true;
  return false;
}

function filterEntries(entries: HistoryEntry[], query: string, status: StatusFilter, date: DateFilter): HistoryEntry[] {
  const q = query.trim().toLowerCase();
  const now = Date.now() / 1000;
  const cutoff = date === "today" ? now - 24 * 3600 : date === "week" ? now - 7 * 24 * 3600 : 0;
  return entries.filter((entry) => {
    if (cutoff && entry.timestamp < cutoff) return false;
    if (status !== "all") {
      const kind = aiStatusKind(entry);
      if (status === "processed" && kind !== "processed") return false;
      if (status === "fallback" && kind !== "fallback") return false;
      if (status === "skipped" && kind !== "skipped" && kind !== "none") return false;
    }
    if (!q) return true;
    const haystack = [entry.text, entry.formatted_text, entry.raw_text, entry.system_prompt, entry.transcription_model]
      .filter(Boolean).join("\n").toLowerCase();
    return haystack.includes(q);
  });
}

function groupByDay(entries: HistoryEntry[]): Array<{ key: string; label: string; entries: HistoryEntry[] }> {
  const groups: Array<{ key: string; label: string; entries: HistoryEntry[] }> = [];
  for (const entry of entries) {
    const d = new Date(entry.timestamp * 1000);
    const key = `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
    let bucket = groups.find((g) => g.key === key);
    if (!bucket) {
      bucket = { key, label: dayBucketLabel(entry.timestamp), entries: [] };
      groups.push(bucket);
    }
    bucket.entries.push(entry);
  }
  return groups;
}

/** The caption under the heading: what exactly is kept right now. 0 means no
 *  limit. */
function describeRetention(maxAgeSeconds: number, maxEntries: number): string {
  const parts: string[] = [];
  if (maxAgeSeconds > 0) {
    const days = Math.round(maxAgeSeconds / 86400);
    // Under a day the count is in hours after all, otherwise it reads "0 days".
    parts.push(days >= 1
      ? tPlural(days, ["{count} день", "{count} дня", "{count} дней"])
      : t("{p0} ч", { p0: Math.round(maxAgeSeconds / 3600) }));
  }
  if (maxEntries > 0) parts.push(tPlural(maxEntries, ["{count} запись", "{count} записи", "{count} записей"]));
  if (parts.length === 0) return t("Хранится без ограничений");
  return t("Хранится {p0}", { p0: parts.join(t(", не больше ")) });
}

export function HistoryPage() {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  // Matches RetentionPolicy::default in src-tauri/src/history.rs.
  const [maxAgeSeconds, setMaxAgeSeconds] = useState(30 * 24 * 3600);
  const [maxEntries, setMaxEntries] = useState(1000);
  const [loading, setLoading] = useState(true);
  const [copiedId, setCopiedId] = useState<number | null>(null);
  const [copiedBlockKey, setCopiedBlockKey] = useState<string | null>(null);
  const [expandedBlockKeys, setExpandedBlockKeys] = useState<Set<string>>(() => new Set());
  const [expandedDetailIds, setExpandedDetailIds] = useState<Set<number>>(() => new Set());
  // Manual LLM processing: one panel at a time, and everything it produced
  // lives here until it is either stored or dropped.
  const [reprocessId, setReprocessId] = useState<number | null>(null);
  const [reprocessProfileId, setReprocessProfileId] = useState<string>("");
  const [reprocessRunning, setReprocessRunning] = useState(false);
  const [reprocessApplying, setReprocessApplying] = useState(false);
  const [reprocessPreview, setReprocessPreview] = useState<HistoryAiPreview | null>(null);
  const [reprocessError, setReprocessError] = useState<string | null>(null);
  // Which run the panel is waiting for. A ref rather than state: it is read
  // inside an awaited closure, where a state variable would still hold the
  // value it had when the request left.
  const reprocessRunRef = useRef(0);
  const [currentAiConfig, setCurrentAiConfig] = useState<AiConfig | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const [viewMode, setViewMode] = useState<ViewMode>("cards");
  const [groupedByDay, setGroupedByDay] = useState<boolean>(true);
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [dateFilter, setDateFilter] = useState<DateFilter>("all");

  const [selectedIds, setSelectedIds] = useState<Set<number>>(() => new Set());
  const [openMenuId, setOpenMenuId] = useState<number | null>(null);
  const [diffEntryIds, setDiffEntryIds] = useState<Set<number>>(() => new Set());
  const [freshIds, setFreshIds] = useState<Set<number>>(() => new Set());

  const seenIdsRef = useRef<Set<number>>(new Set());
  const initializedRef = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const [result, config] = await Promise.all([
        listHistory(),
        invoke<ConfigResult>("get_config"),
      ]);
      const nextEntries = result.entries ?? [];
      setEntries(nextEntries);
      setMaxAgeSeconds(result.max_age_seconds ?? 30 * 24 * 3600);
      setMaxEntries(result.max_entries ?? 1000);
      setCurrentAiConfig(config.ai_processing ?? null);
      setError(null);

      const currentIds = new Set(nextEntries.map((e) => e.id));
      if (!initializedRef.current) {
        seenIdsRef.current = currentIds;
        initializedRef.current = true;
      } else {
        const fresh: number[] = [];
        for (const id of currentIds) if (!seenIdsRef.current.has(id)) fresh.push(id);
        seenIdsRef.current = currentIds;
        if (fresh.length) {
          setFreshIds((prev) => {
            const next = new Set(prev);
            for (const id of fresh) next.add(id);
            return next;
          });
          window.setTimeout(() => {
            setFreshIds((prev) => {
              if (prev.size === 0) return prev;
              const next = new Set(prev);
              for (const id of fresh) next.delete(id);
              return next;
            });
          }, 4500);
        }
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    let unlisten: (() => void) | null = null;
    on<unknown>("history-updated", () => { void refresh(); }).then((fn) => { unlisten = fn; });
    const tick = window.setInterval(() => { void refresh(); }, 30_000);
    return () => {
      unlisten?.();
      window.clearInterval(tick);
    };
  }, [refresh]);

  // Close any open actions menu when clicking outside.
  useEffect(() => {
    if (openMenuId === null) return;
    function onDocClick(ev: MouseEvent) {
      const target = ev.target as HTMLElement | null;
      if (target?.closest?.("[data-menu-root]")) return;
      setOpenMenuId(null);
    }
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, [openMenuId]);

  // Drop selection / an open panel for entries that disappeared (TTL expiry,
  // delete).
  useEffect(() => {
    const ids = new Set(entries.map((e) => e.id));
    setSelectedIds((prev) => {
      let changed = false;
      const next = new Set<number>();
      for (const id of prev) {
        if (ids.has(id)) next.add(id);
        else changed = true;
      }
      return changed ? next : prev;
    });
    if (reprocessId !== null && !ids.has(reprocessId)) setReprocessTarget(null);
  }, [entries, reprocessId]);

  function flashNotice(text: string) {
    setNotice(text);
    window.setTimeout(() => setNotice((current) => current === text ? null : current), 2200);
  }

  async function handleCopy(entry: HistoryEntry) {
    const ok = await copyToClipboard(entry.text);
    if (ok) {
      setCopiedId(entry.id);
      window.setTimeout(() => setCopiedId((current) => current === entry.id ? null : current), 1400);
    } else {
      setError(t("Не удалось скопировать. Скопируйте текст вручную."));
    }
  }

  async function handleCopyBlock(key: string, text: string) {
    const ok = await copyToClipboard(text);
    if (ok) {
      setCopiedBlockKey(key);
      window.setTimeout(() => setCopiedBlockKey((current) => current === key ? null : current), 1400);
    } else {
      setError(t("Не удалось скопировать. Скопируйте текст вручную."));
    }
  }

  function toggleBlock(key: string) {
    setExpandedBlockKeys((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  function toggleDetails(id: number) {
    setExpandedDetailIds((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function toggleDiff(id: number) {
    setDiffEntryIds((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function toggleSelected(id: number) {
    setSelectedIds((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function selectVisible(ids: number[]) {
    setSelectedIds((current) => {
      const next = new Set(current);
      for (const id of ids) next.add(id);
      return next;
    });
  }

  function clearSelection() {
    setSelectedIds(new Set());
  }

  async function handleDelete(entry: HistoryEntry) {
    if (!await confirmDestructive(t("Удалить эту запись? Это действие нельзя отменить."))) return;
    try {
      await deleteHistoryEntry(entry.id);
      setEntries((current) => current.filter((e) => e.id !== entry.id));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleBulkDelete() {
    const ids = Array.from(selectedIds);
    if (!ids.length) return;
    if (!await confirmDestructive(t("Удалить выбранные записи ({p0})? Это действие нельзя отменить.", { p0: ids.length }))) return;
    // `allSettled`, not `all`: a single rejection used to skip the merge
    // entirely, leaving every row that *was* deleted on screen until the next
    // reload — the list then disagreed with the database about what exists.
    const results = await Promise.allSettled(ids.map((id) => deleteHistoryEntry(id)));
    const gone = new Set(ids.filter((_, index) => results[index].status === "fulfilled"));
    setEntries((current) => current.filter((e) => !gone.has(e.id)));
    setSelectedIds((current) => new Set(Array.from(current).filter((id) => !gone.has(id))));
    const failed = results.find((result) => result.status === "rejected");
    if (failed) {
      const reason = (failed as PromiseRejectedResult).reason;
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  async function handleBulkCopy() {
    const ids = selectedIds;
    if (!ids.size) return;
    const ordered = entries.filter((e) => ids.has(e.id));
    const text = ordered.map((e) => e.text).join("\n\n---\n\n");
    const ok = await copyToClipboard(text);
    if (ok) flashNotice(t("Скопировано записей: {p0}", { p0: ordered.length }));
    else setError(t("Не удалось скопировать."));
  }

  // The panel opens on the profile the entry was processed with, when that
  // profile still exists, and on the dictation one otherwise. An id no profile
  // answers to is not passed on: Rust refuses an unknown one outright, and
  // `active_profile_id` outlives the list it points into — `DEFAULT_AI` fills
  // it in as «default» on a config that has no profiles at all. Empty means
  // «the flat fields», which is what those configs actually run on.
  function openReprocess(entry: HistoryEntry) {
    const profiles = currentAiConfig?.profiles ?? [];
    const known = (id: string) => (profiles.some((profile) => profile.id === id) ? id : "");
    setReprocessTarget(entry.id);
    setReprocessProfileId(known(entry.ai_processing?.profile_id ?? "") || known(currentAiConfig?.active_profile_id ?? ""));
  }

  function closeReprocess() {
    setReprocessTarget(null);
  }

  /// Choosing another profile throws the preview away. It describes the run of
  /// the profile that produced it, and the picker above it would already be
  /// naming a different one — «Заменить текст» would then store A's answer
  /// while the panel said B. A run in flight goes with it, for the same reason.
  function chooseReprocessProfile(id: string) {
    reprocessRunRef.current += 1;
    setReprocessProfileId(id);
    setReprocessPreview(null);
    setReprocessError(null);
    setReprocessRunning(false);
  }

  /// Opening, closing and switching panels all invalidate whatever is in
  /// flight: a run belongs to the panel it was started from, and its answer
  /// arrives seconds later, by which time that panel may be showing another
  /// entry. Applying a preview that outlived its panel would write one entry's
  /// LLM output onto a different row.
  function setReprocessTarget(id: number | null) {
    reprocessRunRef.current += 1;
    setReprocessId(id);
    setReprocessPreview(null);
    setReprocessError(null);
    setReprocessRunning(false);
    setReprocessApplying(false);
  }

  async function runReprocess(entry: HistoryEntry) {
    const run = ++reprocessRunRef.current;
    const current = () => reprocessRunRef.current === run;
    setReprocessRunning(true);
    setReprocessError(null);
    setReprocessPreview(null);
    try {
      const preview = await previewHistoryAiProcessing(
        entry.id,
        reprocessProfileId || undefined,
        reprocessProfileId ? reprocessPrompt(currentAiConfig, reprocessProfileId) : undefined,
      );
      if (!current()) return;
      if (preview.ok) {
        setReprocessPreview(preview);
        return;
      }
      // A refusal stays in the panel and nothing is written: the row keeps
      // describing the last run that actually produced text.
      setReprocessError(preview.reason
        ? t("LLM не вернула текст: {p0}", { p0: aiSkipLabel(preview.reason) })
        : t("LLM не вернула текст."));
    } catch (e) {
      if (!current()) return;
      setReprocessError(e instanceof Error ? e.message : String(e));
    } finally {
      if (current()) setReprocessRunning(false);
    }
  }

  async function applyReprocess(entry: HistoryEntry) {
    const preview = reprocessPreview;
    if (!preview) return;
    // The row is merged either way — the write has happened, and the entry it
    // happened to is the one this call captured. Only the panel's own state is
    // conditional: by the time the answer comes back it may be somebody else's
    // panel, and closing it would take away a preview nobody had ruled on.
    const run = reprocessRunRef.current;
    const stillOpen = () => reprocessRunRef.current === run;
    setReprocessApplying(true);
    setReprocessError(null);
    try {
      const result = await applyHistoryAiProcessing(entry.id, preview.text, preview.ai_json, preview.stats_json);
      const updated = result.entry;
      if (!result.updated || !updated) {
        if (stillOpen()) setReprocessError(t("Не удалось сохранить результат."));
        return;
      }
      setEntries((current) => current.map((item) => item.id === updated.id ? updated : item));
      if (stillOpen()) closeReprocess();
      flashNotice(t("Текст заменен результатом LLM"));
    } catch (e) {
      if (stillOpen()) setReprocessError(e instanceof Error ? e.message : String(e));
    } finally {
      if (stillOpen()) setReprocessApplying(false);
    }
  }

  async function handleClearAll() {
    if (!await confirmDestructive(t("Очистить всю историю? Это действие нельзя отменить."), t("Очистить"))) return;
    try {
      await clearHistory();
      setEntries([]);
      clearSelection();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  const filtered = useMemo(
    () => filterEntries(entries, query, statusFilter, dateFilter),
    [entries, query, statusFilter, dateFilter],
  );
  const groups = useMemo(
    () => groupedByDay
      ? groupByDay(filtered)
      : (filtered.length ? [{ key: "all", label: "", entries: filtered }] : []),
    [filtered, groupedByDay],
  );

  const filterIsActive = query.trim().length > 0 || statusFilter !== "all" || dateFilter !== "all";
  const retentionText = describeRetention(maxAgeSeconds, maxEntries);
  const subtitle = filterIsActive
    ? t("Показано {p0} из {p1} · {p2}", { p0: filtered.length, p1: entries.length, p2: retentionText })
    : retentionText;

  const visibleIds = useMemo(() => filtered.map((e) => e.id), [filtered]);
  const allVisibleSelected = visibleIds.length > 0 && visibleIds.every((id) => selectedIds.has(id));

  function resetFilters() {
    setQuery("");
    setStatusFilter("all");
    setDateFilter("all");
  }

  return (
    <div className="page">
      <PageHeader
        title={t("История транскрипций")}
        sub={subtitle}
        actions={
          <>
            <Segmented value={viewMode} onChange={(v) => setViewMode(v as ViewMode)} options={[
              { value: "cards", label: t("Карточки") },
              { value: "list", label: t("Список") },
            ]}/>
            <Hint text={groupedByDay ? t("Выключить группировку по дням") : t("Группировать по дням")}>
              <button className="btn btn--ghost" onClick={() => setGroupedByDay((v) => !v)} aria-pressed={groupedByDay}>
                <Icon name="clock" size={12}/> {groupedByDay ? t("По дням") : t("Без групп")}
              </button>
            </Hint>
            <button className="btn btn--ghost" onClick={() => void handleClearAll()} disabled={entries.length === 0}>
              <Icon name="trash" size={12}/>  {t("Очистить всё")} </button>
          </>
        }
      />
      <div style={{ display: "grid", gap: 12 }}>
        {error && (
          <div role="alert" style={{ padding: "10px 12px", borderRadius: 8, background: "rgba(239,94,107,0.12)", border: "1px solid rgba(239,94,107,0.35)", color: "var(--err)", font: "500 12px/1.35 var(--font-sans)" }}>
            {error}
            <button className="btn btn--ghost" style={{ marginLeft: 8, height: 22 }} onClick={() => setError(null)}><Icon name="x" size={10}/>{t("Скрыть")}</button>
          </div>
        )}
        {notice && (
          <div role="status" aria-live="polite" style={{ padding: "10px 12px", borderRadius: 8, background: "var(--accent-soft-2)", border: "1px solid var(--accent-soft-2)", color: "var(--ink)", font: "500 12px/1.35 var(--font-sans)" }}>{notice}</div>
        )}

        {entries.length > 0 && (
          <FiltersBar
            query={query}
            onQuery={setQuery}
            statusFilter={statusFilter}
            onStatusFilter={setStatusFilter}
            dateFilter={dateFilter}
            onDateFilter={setDateFilter}
            onReset={resetFilters}
            filterIsActive={filterIsActive}
          />
        )}

        {selectedIds.size > 0 && (
          <BulkBar
            count={selectedIds.size}
            totalVisible={visibleIds.length}
            allVisibleSelected={allVisibleSelected}
            onSelectAllVisible={() => selectVisible(visibleIds)}
            onClear={clearSelection}
            onCopy={() => void handleBulkCopy()}
            onDelete={() => void handleBulkDelete()}
          />
        )}

        {loading && entries.length === 0 ? (
          <EmptyState icon="clock" title={t("Загружаю…")} hint=""/>
        ) : entries.length === 0 ? (
          <EmptyState
            icon="clock"
            title={t("История пуста")}
            hint={t("Здесь будут появляться последние транскрипции. Хранятся локально, не покидают этот компьютер.")}
          />
        ) : filtered.length === 0 ? (
          <EmptyState
            icon="search"
            title={t("Ничего не найдено")}
            hint={t("Попробуйте изменить запрос или сбросить фильтры.")}
          />
        ) : (
          <div style={{ display: "grid", gap: groupedByDay ? 16 : 10 }}>
            {groups.map((group) => (
              <section key={group.key} style={{ display: "grid", gap: 8 }}>
                {group.label && (
                  <div className="flex-row" style={{ gap: 8, padding: "0 4px", margin: "4px 0 2px" }}>
                    <h2 style={{ margin: 0, font: "500 10.5px/1 var(--font-mono)", color: "var(--ink-mute)", textTransform: "uppercase", letterSpacing: "0.08em" }}>{group.label}</h2>
                    <span className="head-count">{group.entries.length}</span>
                  </div>
                )}
                <div style={{ display: "grid", gap: viewMode === "list" ? 4 : 10 }}>
                  {group.entries.map((entry) => (
                    <EntryCard
                      key={entry.id}
                      entry={entry}
                      viewMode={viewMode}
                      selected={selectedIds.has(entry.id)}
                      onToggleSelected={() => toggleSelected(entry.id)}
                      detailsExpanded={expandedDetailIds.has(entry.id)}
                      onToggleDetails={() => toggleDetails(entry.id)}
                      diffOn={diffEntryIds.has(entry.id)}
                      onToggleDiff={() => toggleDiff(entry.id)}
                      fresh={freshIds.has(entry.id)}
                      copiedId={copiedId}
                      onCopy={() => void handleCopy(entry)}
                      onDelete={() => void handleDelete(entry)}
                      currentAiConfig={currentAiConfig}
                      copiedBlockKey={copiedBlockKey}
                      onCopyBlock={handleCopyBlock}
                      expandedBlockKeys={expandedBlockKeys}
                      onToggleBlock={toggleBlock}
                      reprocessOpen={reprocessId === entry.id}
                      onOpenReprocess={() => openReprocess(entry)}
                      onCloseReprocess={closeReprocess}
                      reprocessProfileId={reprocessProfileId}
                      onReprocessProfileId={chooseReprocessProfile}
                      reprocessRunning={reprocessRunning}
                      reprocessApplying={reprocessApplying}
                      reprocessPreview={reprocessPreview}
                      reprocessError={reprocessError}
                      onRunReprocess={() => void runReprocess(entry)}
                      onApplyReprocess={() => void applyReprocess(entry)}
                      menuOpen={openMenuId === entry.id}
                      onToggleMenu={() => setOpenMenuId((current) => current === entry.id ? null : entry.id)}
                    />
                  ))}
                </div>
              </section>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function FiltersBar({
  query, onQuery, statusFilter, onStatusFilter, dateFilter, onDateFilter, onReset, filterIsActive,
}: {
  query: string;
  onQuery: (v: string) => void;
  statusFilter: StatusFilter;
  onStatusFilter: (v: StatusFilter) => void;
  dateFilter: DateFilter;
  onDateFilter: (v: DateFilter) => void;
  onReset: () => void;
  filterIsActive: boolean;
}) {
  return (
    <Card pad="rows">
      <div className="flex-row" style={{ gap: 12, flexWrap: "wrap", alignItems: "center" }}>
        <label className="input-search" style={{ flex: "1 1 260px", minWidth: 200 }}>
          <span className="input-search__icon"><Icon name="search" size={13}/></span>
          <input
            className="field"
            type="search"
            value={query}
            onChange={(e) => onQuery(e.target.value)}
            placeholder={t("Поиск по тексту, raw и системному промпту")}
            aria-label={t("Поиск по истории")}
            style={{ width: "100%", height: 30 }}
          />
          {query && (
            <button
              type="button"
              onClick={() => onQuery("")}
              aria-label={t("Очистить поиск")}
              className="btn btn--ghost"
              style={{ position: "absolute", right: 4, top: "50%", transform: "translateY(-50%)", height: 22, padding: "0 6px" }}
            >
              <Icon name="x" size={10}/>
            </button>
          )}
        </label>
        <Segmented
          value={statusFilter}
          onChange={(v) => onStatusFilter(v as StatusFilter)}
          options={[
            { value: "all", label: t("Все") },
            { value: "processed", label: t("Обработано") },
            { value: "fallback", label: "Fallback" },
            { value: "skipped", label: t("Без LLM") },
          ]}
        />
        <Segmented
          value={dateFilter}
          onChange={(v) => onDateFilter(v as DateFilter)}
          options={[
            { value: "all", label: t("За всё время") },
            { value: "today", label: t("Сегодня") },
            { value: "week", label: t("Неделя") },
          ]}
        />
        {filterIsActive && (
          <Hint text={t("Сбросить фильтры")}>
            <button className="btn btn--ghost" onClick={onReset}><Icon name="x" size={11}/>  {t("Сбросить")}</button>
          </Hint>
        )}
      </div>
    </Card>
  );
}

function BulkBar({ count, totalVisible, allVisibleSelected, onSelectAllVisible, onClear, onCopy, onDelete }: {
  count: number;
  totalVisible: number;
  allVisibleSelected: boolean;
  onSelectAllVisible: () => void;
  onClear: () => void;
  onCopy: () => void;
  onDelete: () => void;
}) {
  return (
    <div
      role="region"
      aria-label={t("Действия с выбранными записями")}
      style={{
        position: "sticky", top: 0, zIndex: 5,
        display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center",
        padding: "8px 12px", borderRadius: "var(--radius-sm)",
        background: "var(--accent-soft-2)", border: "1px solid var(--accent-soft-2)",
        boxShadow: "0 2px 12px rgba(0,0,0,0.12)",
      }}
    >
      <span style={{ font: "600 12px/1 var(--font-sans)", color: "var(--ink)" }}>{t("Выбрано:")} {count}</span>
      {!allVisibleSelected && totalVisible > count && (
        <button className="btn btn--ghost" onClick={onSelectAllVisible}><Icon name="check" size={11}/>{t("Выделить видимые ({p0})", { p0: totalVisible })}</button>
      )}
      <button className="btn btn--ghost" onClick={onCopy}><Icon name="copy" size={11}/>{t("Скопировать")}</button>
      <button className="btn btn--ghost" onClick={onDelete}><Icon name="trash" size={11}/>{t("Удалить")}</button>
      <button className="btn btn--ghost" style={{ marginLeft: "auto" }} onClick={onClear}><Icon name="x" size={11}/>{t("Сбросить")}</button>
    </div>
  );
}

function EntryCard(props: {
  entry: HistoryEntry;
  viewMode: ViewMode;
  selected: boolean;
  onToggleSelected: () => void;
  detailsExpanded: boolean;
  onToggleDetails: () => void;
  diffOn: boolean;
  onToggleDiff: () => void;
  fresh: boolean;
  copiedId: number | null;
  onCopy: () => void;
  onDelete: () => void;
  currentAiConfig: AiConfig | null;
  copiedBlockKey: string | null;
  onCopyBlock: (key: string, text: string) => void;
  expandedBlockKeys: Set<string>;
  onToggleBlock: (key: string) => void;
  reprocessOpen: boolean;
  onOpenReprocess: () => void;
  onCloseReprocess: () => void;
  reprocessProfileId: string;
  onReprocessProfileId: (id: string) => void;
  reprocessRunning: boolean;
  reprocessApplying: boolean;
  reprocessPreview: HistoryAiPreview | null;
  reprocessError: string | null;
  onRunReprocess: () => void;
  onApplyReprocess: () => void;
  menuOpen: boolean;
  onToggleMenu: () => void;
}) {
  const {
    entry, viewMode, selected, onToggleSelected, detailsExpanded, onToggleDetails,
    diffOn, onToggleDiff, fresh, copiedId, onCopy, onDelete,
    currentAiConfig,
    copiedBlockKey, onCopyBlock, expandedBlockKeys, onToggleBlock,
    reprocessOpen, onOpenReprocess, onCloseReprocess, reprocessProfileId, onReprocessProfileId,
    reprocessRunning, reprocessApplying, reprocessPreview, reprocessError,
    onRunReprocess, onApplyReprocess, menuOpen, onToggleMenu,
  } = props;

  const compact = viewMode === "list" && !detailsExpanded;
  const formattedKey = `${entry.id}:formatted`;
  const rawKey = `${entry.id}:raw`;
  const canReprocess = reprocessSource(entry).length > 0;
  const hasDetails = entryHasDetails(entry);
  const canDiff = !!(entry.formatted_text && entry.formatted_text !== entry.text);
  const aiBadgeColor = aiStatusColor(entry);
  const profileLabel = aiProfileLabel(entry);
  const sttLabel = transcriptionModelLabel(entry);

  const borderStyle = selected
    ? "1px solid var(--accent-soft-2)"
    : fresh ? "1px solid var(--accent-soft-2)" : "1px solid var(--line)";
  const cardBackground = selected
    ? "var(--accent-soft-2)"
    : fresh ? "linear-gradient(180deg, var(--accent-soft-2), var(--bg-2))" : "var(--bg-2)";

  return (
    <article
      style={{
        display: "grid",
        gridTemplateColumns: "auto 1fr auto",
        gap: 10,
        padding: compact ? "8px 10px" : 12,
        borderRadius: "var(--radius-sm)",
        background: cardBackground,
        border: borderStyle,
        alignItems: "start",
        transition: "background 200ms ease, border-color 200ms ease",
      }}
    >
      <input
        className="checkbox"
        type="checkbox"
        checked={selected}
        onChange={onToggleSelected}
        aria-label={t("Выбрать запись от {p0}", { p0: formatTime(entry.timestamp) })}
        style={{ marginTop: 4 }}
      />
      <div style={{ minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", marginBottom: compact ? 4 : 6 }}>
          <span
            title={aiStatusText(entry)}
            aria-label={aiStatusText(entry)}
            style={{
              width: 8, height: 8, borderRadius: "50%",
              background: aiBadgeColor,
              flexShrink: 0,
              boxShadow: aiStatusKind(entry) === "processed" ? "0 0 0 3px color-mix(in srgb, var(--ok) 18%, transparent)" : undefined,
            }}
          />
          <span style={{ font: "500 11px/1 var(--font-mono)", color: "var(--ink-mute)", letterSpacing: "0.04em" }}>{formatTime(entry.timestamp)}</span>
          <span style={{ font: "400 11px/1 var(--font-sans)", color: "var(--ink-faint)" }}>· {relativeAge(entry.timestamp)}</span>
          <span style={{ font: "500 11px/1 var(--font-mono)", color: "var(--ink-mute)" }} title={t("Модель первичной транскрибации: {p0}", { p0: sttLabel })}>
            · {t("STT: {p0}", { p0: sttLabel })}
          </span>
          {profileLabel && (
            <span style={{ font: "500 11px/1 var(--font-mono)", color: "var(--ink-mute)" }} title={aiStatusText(entry)}>· {t("AI: {p0}", { p0: profileLabel })}</span>
          )}
          {fresh && <span className="tag" style={{ height: 18, fontSize: 9, background: "var(--accent-soft-2)", borderColor: "var(--accent-soft-2)", color: "var(--ink)" }}>{t("новое")}</span>}
        </div>

        {compact ? (
          <Hint text={t("Развернуть")} className="hint-anchor--block">
            <div
              onClick={onToggleDetails}
              style={{ font: "400 13px/1.4 var(--font-sans)", color: "var(--ink)", display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden", cursor: "pointer" }}
            >{entry.text}</div>
          </Hint>
        ) : (
          <>
            <div
              title={t("{p0} симв.", { p0: entry.length })}
              style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--ink)", whiteSpace: "pre-wrap", overflowWrap: "break-word" }}
            >
              {entry.text}
            </div>
            {(hasDetails || canDiff || canReprocess) && (
              <div style={{ marginTop: 10, display: "flex", flexWrap: "wrap", gap: 6 }}>
                {hasDetails && (
                  <button className="btn btn--ghost" onClick={onToggleDetails} aria-expanded={detailsExpanded} style={{ height: 24 }}>
                    <Icon name={detailsExpanded ? "chev-down" : "chev"} size={11} style={{ transform: detailsExpanded ? undefined : "rotate(90deg)" }}/>
                    {detailsExpanded ? t("Скрыть детали") : t("Подробнее")}
                  </button>
                )}
                {canDiff && (
                  <button className="btn btn--ghost" onClick={onToggleDiff} aria-pressed={diffOn} style={{ height: 24 }}>
                    <Icon name="compare" size={11}/>{diffOn ? t("Скрыть diff") : t("Сравнить с до-LLM")}
                  </button>
                )}
                {canReprocess && (
                  <button
                    className="btn btn--ghost"
                    onClick={reprocessOpen ? onCloseReprocess : onOpenReprocess}
                    aria-expanded={reprocessOpen}
                    style={{ height: 24 }}
                  >
                    <Icon name="wand" size={11}/>{t("Обработать через LLM")}
                  </button>
                )}
              </div>
            )}
            {diffOn && canDiff && (
              <DiffBlock before={entry.formatted_text || ""} after={entry.text}/>
            )}
            {reprocessOpen && (
              <ReprocessPanel
                entry={entry}
                aiConfig={currentAiConfig}
                profileId={reprocessProfileId}
                onProfileId={onReprocessProfileId}
                running={reprocessRunning}
                applying={reprocessApplying}
                preview={reprocessPreview}
                error={reprocessError}
                onRun={onRunReprocess}
                onApply={onApplyReprocess}
              />
            )}
            {detailsExpanded && (
              <div style={{ marginTop: 10, display: "grid", gap: 0 }}>
                <StatsGrid entry={entry}/>
                {entry.ai_processing?.provider_error && (
                  <HistoryTextBlock
                    title={t("Причина fallback")}
                    text={entry.ai_processing.provider_error}
                    muted
                    copied={copiedBlockKey === `${entry.id}:provider_error`}
                    onCopy={() => onCopyBlock(`${entry.id}:provider_error`, entry.ai_processing?.provider_error ?? "")}
                  />
                )}
                {entry.formatted_text && entry.formatted_text !== entry.text && (
                  <HistoryTextBlock
                    title={t("До LLM, после локальной обработки")}
                    text={entry.formatted_text}
                    muted
                    collapsible
                    collapsed={!expandedBlockKeys.has(formattedKey)}
                    copied={copiedBlockKey === formattedKey}
                    onToggle={() => onToggleBlock(formattedKey)}
                    onCopy={() => onCopyBlock(formattedKey, entry.formatted_text ?? "")}
                  />
                )}
                {entry.raw_text && entry.raw_text !== entry.formatted_text && entry.raw_text !== entry.text && (
                  <HistoryTextBlock
                    title={t("Распознавание без обработки")}
                    text={entry.raw_text}
                    muted
                    collapsible
                    collapsed={!expandedBlockKeys.has(rawKey)}
                    copied={copiedBlockKey === rawKey}
                    onToggle={() => onToggleBlock(rawKey)}
                    onCopy={() => onCopyBlock(rawKey, entry.raw_text ?? "")}
                  />
                )}
              </div>
            )}
          </>
        )}
      </div>

      <div style={{ display: "flex", gap: 4, alignItems: "start" }}>
        <Hint text={copiedId === entry.id ? t("Скопировано") : t("Скопировать в буфер обмена")}>
          <button
            className={copiedId === entry.id ? "btn btn--primary" : "btn btn--ghost"}
            onClick={onCopy}
            aria-label={t("Скопировать")}
            style={{ height: 30, padding: "0 9px" }}
          >
            <Icon name={copiedId === entry.id ? "check" : "copy"} size={15}/>
          </button>
        </Hint>
        <ActionsMenu
          open={menuOpen}
          onToggle={onToggleMenu}
          actions={[
            { label: t("Удалить"), icon: "trash", onClick: onDelete, danger: true },
          ]}
        />
      </div>
    </article>
  );
}

function ActionsMenu({ open, onToggle, actions }: {
  open: boolean;
  onToggle: () => void;
  actions: Array<{ label: string; icon: string; onClick: () => void; disabled?: boolean; danger?: boolean }>;
}) {
  return (
    <div data-menu-root style={{ position: "relative" }}>
      <button
        className="btn btn--ghost"
        onClick={onToggle}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={t("Действия")}
        style={{ height: 30, padding: "0 9px" }}
      >
        <Icon name="more" size={15}/>
      </button>
      {open && (
        <div
          role="menu"
          style={{
            position: "absolute",
            right: 0,
            top: "calc(100% + 4px)",
            minWidth: 100,
            padding: 4,
            background: "var(--bg-3)",
            border: "1px solid var(--line-strong)",
            borderRadius: "var(--radius-sm)",
            boxShadow: "0 8px 24px rgba(0,0,0,0.25)",
            zIndex: 10,
            display: "grid",
            gap: 2,
          }}
        >
          {actions.map((action) => (
            <button
              key={action.label}
              role="menuitem"
              disabled={action.disabled}
              onClick={action.onClick}
              style={{
                appearance: "none",
                cursor: action.disabled ? "not-allowed" : "pointer",
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "6px 8px",
                border: 0,
                borderRadius: 4,
                background: "transparent",
                color: action.danger ? "var(--err)" : "var(--ink)",
                font: "500 12px/1.1 var(--font-sans)",
                textAlign: "left",
                opacity: action.disabled ? 0.5 : 1,
              }}
              onMouseEnter={(ev) => { (ev.currentTarget as HTMLButtonElement).style.background = "var(--bg-4)"; }}
              onMouseLeave={(ev) => { (ev.currentTarget as HTMLButtonElement).style.background = "transparent"; }}
            >
              <Icon name={action.icon} size={12}/>{action.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** Profiles to choose between: the name, and under it the model it actually
 *  calls — a profile name alone does not say where the text is going.
 *
 *  The empty id is not a profile but the route the flat fields describe, and it
 *  needs a row of its own all the same: it is what a config with no profiles
 *  runs on, and `CustomSelect` renders nothing at all when the value it is
 *  given matches no option. */
export function reprocessProfileOptions(aiConfig: AiConfig | null, value: string): Array<{ value: string; label: string; meta?: string }> {
  const profiles: AiProfile[] = aiConfig?.profiles ?? [];
  const options = profiles.map((profile) => ({
    value: profile.id,
    label: profile.name,
    meta: profile.model?.trim() || undefined,
  }));
  if (options.some((option) => option.value === value)) return options;
  return [{ value, label: aiTargetText(aiConfig) }, ...options];
}

/**
 * Manual LLM processing of one entry: pick a profile, run it, keep the result
 * or leave it.
 *
 * The run and the write are separate steps — «Прогнать» only asks the model,
 * and the history changes on «Заменить текст». The control this replaced
 * overwrote the row on the first click, with no way back and no way to see
 * what changed.
 */
function ReprocessPanel({
  entry, aiConfig, profileId, onProfileId, running, applying, preview, error, onRun, onApply,
}: {
  entry: HistoryEntry;
  aiConfig: AiConfig | null;
  profileId: string;
  onProfileId: (id: string) => void;
  running: boolean;
  applying: boolean;
  preview: HistoryAiPreview | null;
  error: string | null;
  onRun: () => void;
  onApply: () => void;
}) {
  const busy = running || applying;
  return (
    <div style={{ marginTop: 10, display: "grid", gap: 8 }}>
      <div className="flex-row" style={{ gap: 6, flexWrap: "wrap", alignItems: "center" }}>
        <div style={{ flex: "0 1 auto", minWidth: 180 }}>
          <CustomSelect
            value={profileId}
            options={reprocessProfileOptions(aiConfig, profileId)}
            onChange={onProfileId}
            disabled={busy}
            inlineMeta
            metaSeparator="dash"
          />
        </div>
        <button className="btn btn--primary" onClick={onRun} disabled={busy} aria-busy={running} style={{ height: "var(--control-h)" }}>
          {running && <span className="mini-spinner" aria-hidden="true"/>}
          {running ? t("Обрабатываю…") : t("Запустить")}
        </button>
      </div>

      {error && (
        <div role="alert" style={{ font: "500 11.5px/1.4 var(--font-sans)", color: "var(--err)" }}>{error}</div>
      )}

      {preview && (
        <div style={{ display: "grid", gap: 8 }}>
          <DiffBlock before={entry.text} after={preview.text} title={t("Diff: сейчас → новый вариант")}/>
          <div>
            <button className="btn btn--primary" onClick={onApply} disabled={busy} aria-busy={applying} style={{ height: 26 }}>
              {applying ? <span className="mini-spinner" aria-hidden="true"/> : <Icon name="check" size={11}/>}
              {applying ? t("Сохраняю") : t("Заменить текст")}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function StatTile({ label, value }: { label: string; value: string }) {
  return (
    <div style={{
      display: "grid",
      gap: 3,
      padding: "8px 10px",
      background: "var(--bg-3)",
      borderRadius: "var(--radius-sm)",
      border: "1px solid var(--line)",
      minWidth: 0,
    }}>
      <span style={{ font: "600 12.5px/1 var(--font-mono)", color: "var(--ink)" }}>{value}</span>
      <span style={{ font: "500 9.5px/1 var(--font-mono)", color: "var(--ink-mute)", textTransform: "uppercase", letterSpacing: "0.05em" }}>{label}</span>
    </div>
  );
}

function StatsGrid({ entry }: { entry: HistoryEntry }) {
  const stats = entry.processing_stats;
  const ai = entry.ai_processing;
  const audioSeconds = stats?.audio_seconds ?? ai?.audio_duration_seconds;
  const tiles: Array<{ label: string; value: number | null | undefined }> = [
    { label: t("Аудио"), value: audioSeconds },
    { label: "STT", value: stats?.whisper_seconds },
    { label: t("Формат"), value: stats?.format_seconds },
    { label: "LLM", value: stats?.llm_seconds },
    { label: t("Всего"), value: stats?.total_seconds },
  ];
  const visible = tiles.filter(({ value }) => typeof value === "number" && Number.isFinite(value));
  const replacements = stats?.replacement_stats?.total ?? 0;
  const chips: string[] = [];
  if (ai?.timeout_seconds) chips.push(t("LLM timeout {p0} с", { p0: ai.timeout_seconds }));
  if (ai?.attempt_timeout_seconds && ai.attempt_timeout_seconds !== ai.timeout_seconds) chips.push(t("попытка {p0} с", { p0: ai.attempt_timeout_seconds }));
  if (ai?.attempts && ai.attempts > 1) chips.push(t("попыток {p0}", { p0: ai.attempts }));
  if (ai?.error_type) chips.push(ai.error_type);

  if (visible.length === 0 && chips.length === 0 && replacements === 0) return null;

  return (
    <div style={{ marginBottom: 10, display: "grid", gap: 8 }}>
      {visible.length > 0 && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(72px, 1fr))", gap: 6 }}>
          {visible.map(({ label, value }) => (
            <StatTile key={label} label={label} value={formatSeconds(value)}/>
          ))}
          {replacements > 0 && <StatTile label={t("Замен")} value={String(replacements)}/>}
        </div>
      )}
      {chips.length > 0 && (
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
          {chips.map((text) => (
            <span key={text} className="tag" style={{ height: 20, fontSize: 10 }}>{text}</span>
          ))}
        </div>
      )}
    </div>
  );
}

function HistoryTextBlock({ title, text, muted = false, collapsible = false, collapsed = false, copied = false, onToggle, onCopy }: { title: string; text: string; muted?: boolean; collapsible?: boolean; collapsed?: boolean; copied?: boolean; onToggle?: () => void; onCopy?: () => void }) {
  return (
    <div style={{ marginTop: muted ? 10 : 0 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: collapsed ? 0 : 4 }}>
        <div style={{ flex: "0 1 auto", minWidth: 0, font: "600 10px/1 var(--font-mono)", color: muted ? "var(--ink-mute)" : "var(--ink-dim)", textTransform: "uppercase", letterSpacing: "0.04em" }}>{title}</div>
        <Hint text={t("Копировать: {p0}", { p0: title })}>
          <button
            className={copied ? "btn btn--primary" : "btn btn--ghost"}
            onClick={onCopy}
            aria-label={t("Копировать блок {p0}", { p0: title })}
            style={{ height: 22, padding: "0 6px" }}
          >
            <Icon name={copied ? "check" : "copy"} size={10}/>
          </button>
        </Hint>
        {collapsible && (
          <Hint text={collapsed ? t("Развернуть блок") : t("Свернуть блок")}>
          <button
            className="btn btn--ghost"
            onClick={onToggle}
            aria-label={collapsed ? t("Развернуть блок {p0}", { p0: title }) : t("Свернуть блок {p0}", { p0: title })}
            aria-expanded={!collapsed}
            style={{ height: 22, padding: "0 6px" }}
          >
            <Icon name={collapsed ? "chev-down" : "chev"} size={10} style={{ transform: collapsed ? undefined : "rotate(90deg)" }}/>
          </button>
          </Hint>
        )}
      </div>
      {!collapsed && (
        <div style={{ font: "400 13px/1.5 var(--font-sans)", color: muted ? "var(--ink-dim)" : "var(--ink)", whiteSpace: "pre-wrap", overflowWrap: "break-word" }}>
          {text}
        </div>
      )}
    </div>
  );
}

function EmptyState({ icon, title, hint }: { icon: string; title: string; hint: string }) {
  return (
    <div style={{ display: "grid", placeItems: "center", padding: "48px 24px", color: "var(--ink-mute)", textAlign: "center" }}>
      <div style={{ marginBottom: 12, opacity: 0.7 }}><Icon name={icon} size={32}/></div>
      <div style={{ font: "500 14px/1.3 var(--font-sans)", color: "var(--ink-dim)" }}>{title}</div>
      {hint && <div style={{ marginTop: 6, maxWidth: 380, font: "400 12px/1.5 var(--font-sans)" }}>{hint}</div>}
    </div>
  );
}
