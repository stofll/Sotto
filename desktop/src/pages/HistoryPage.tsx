import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { confirmDestructive, invoke, on } from "../bridge";
import {
    clearHistory,
    deleteHistoryEntry,
    listHistory,
    retryHistoryAiProcessing,
    updateHistoryEntryText,
} from "../bridge/stats";
import { PageHeader, Segmented } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Hint } from "../components/Hint";
import { DiffBlock } from "../components/DiffBlock";
import type { ConfigResult, HistoryEntry, HistoryRetryAiResult } from "../bridge/types";
import { localeTag, t, tPlural } from "../i18n";

type AiConfig = ConfigResult["ai_processing"];
type ProcessingAction = "process" | "retry";
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
  return "var(--text-mute)";
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

function aiActionTitle(action: ProcessingAction): string {
  return action === "retry" ? t("Повторная LLM-обработка") : t("Первичная LLM-обработка");
}

function canRetryAiProcessing(entry: HistoryEntry): boolean {
  return !!(entry.ai_processing?.attempted && entry.ai_processing?.fallback && (entry.formatted_text || entry.text));
}

function canProcessAiProcessing(entry: HistoryEntry): boolean {
  if (!entry.text?.trim()) return false;
  if (entry.ai_processing?.attempted && entry.ai_processing?.used) return false;
  if (canRetryAiProcessing(entry)) return false;
  return true;
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

function downloadTextFile(filename: string, text: string, mime = "text/markdown;charset=utf-8") {
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

function entryToMarkdown(entry: HistoryEntry): string {
  const date = new Date(entry.timestamp * 1000).toLocaleString(localeTag());
  const lines: string[] = [
    t("# Транскрипция · {p0}", { p0: date }),
    "",
    t("- Модель транскрибации: {p0}", { p0: transcriptionModelLabel(entry) }),
    "",
    entry.text || "",
  ];
  if (entry.formatted_text && entry.formatted_text !== entry.text) {
    lines.push("", t("## До LLM, после локальной обработки"), "", entry.formatted_text);
  }
  if (entry.raw_text && entry.raw_text !== entry.formatted_text && entry.raw_text !== entry.text) {
    lines.push("", t("## Распознавание без обработки"), "", entry.raw_text);
  }
  const ai = entry.ai_processing;
  if (ai && (ai.provider || ai.model)) {
    lines.push("", "## LLM", "", t("- Провайдер: {p0}", { p0: ai.provider ?? "-" }), t("- Модель: {p0}", { p0: ai.model ?? "-" }));
    if (ai.profile_name) lines.push(t("- Профиль: {p0}", { p0: ai.profile_name }));
    if (ai.error_type) lines.push(t("- Ошибка: {p0}", { p0: ai.error_type }));
  }
  return lines.join("\n");
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
  const [retryingId, setRetryingId] = useState<number | null>(null);
  const [processingAction, setProcessingAction] = useState<ProcessingAction | null>(null);
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
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editingText, setEditingText] = useState("");
  const [savingEditId, setSavingEditId] = useState<number | null>(null);
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

  // Drop selection / editing for entries that disappeared (TTL expiry, delete).
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
    if (editingId !== null && !ids.has(editingId)) {
      setEditingId(null);
      setEditingText("");
    }
  }, [entries, editingId]);

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
    try {
      await Promise.all(ids.map((id) => deleteHistoryEntry(id)));
      setEntries((current) => current.filter((e) => !selectedIds.has(e.id)));
      clearSelection();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
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

  // Merge the row back even when the pass failed: it now carries the
  // AiStatus of the attempt that just ran, which is what renders the
  // provider error and the "LLM: пропущено · …" badge inline on the entry.
  // The toast only says that something went wrong; the row says what.
  function mergeRetryResult(result: HistoryRetryAiResult) {
    const updatedEntry = result.entry;
    if (!updatedEntry) return;
    setEntries((current) => current.map((item) => item.id === updatedEntry.id ? updatedEntry : item));
  }

  async function handleRetryAi(entry: HistoryEntry) {
    setRetryingId(entry.id);
    setProcessingAction("retry");
    setError(null);
    try {
      const result = await retryHistoryAiProcessing(entry.id);
      mergeRetryResult(result);
      if (!result.updated) {
        setError(result.reason ? t("Не удалось повторить LLM-обработку: {p0}", { p0: aiSkipLabel(result.reason) }) : t("Не удалось повторить LLM-обработку."));
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRetryingId((current) => current === entry.id ? null : current);
      setProcessingAction(null);
    }
  }

  async function handleProcessAi(entry: HistoryEntry) {
    setRetryingId(entry.id);
    setProcessingAction("process");
    setError(null);
    try {
      // process_history_ai alias — reused via retryHistoryAiProcessing (Rust single entry point).
      const result = await retryHistoryAiProcessing(entry.id);
      mergeRetryResult(result);
      if (!result.updated) {
        setError(result.reason ? t("Не удалось обработать через LLM: {p0}", { p0: aiSkipLabel(result.reason) }) : t("Не удалось обработать через LLM."));
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRetryingId((current) => current === entry.id ? null : current);
      setProcessingAction(null);
    }
  }

  function startEdit(entry: HistoryEntry) {
    setEditingId(entry.id);
    setEditingText(entry.text);
    setOpenMenuId(null);
  }

  function cancelEdit() {
    setEditingId(null);
    setEditingText("");
  }

  async function saveEdit() {
    if (editingId === null) return;
    const trimmed = editingText.trim();
    if (!trimmed) {
      setError(t("Текст не может быть пустым."));
      return;
    }
    setSavingEditId(editingId);
    try {
      const result = await updateHistoryEntryText(editingId, trimmed);
      if (!result.updated || !result.entry) {
        setError(result.reason ? t("Не удалось сохранить: {p0}", { p0: result.reason }) : t("Не удалось сохранить."));
        return;
      }
      const updated = result.entry;
      setEntries((current) => current.map((item) => item.id === updated.id ? updated : item));
      setEditingId(null);
      setEditingText("");
      flashNotice(t("Текст обновлен"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSavingEditId((current) => current === editingId ? null : current);
    }
  }

  function handleExport(entry: HistoryEntry) {
    const date = new Date(entry.timestamp * 1000);
    const stamp = `${date.getFullYear()}${String(date.getMonth() + 1).padStart(2, "0")}${String(date.getDate()).padStart(2, "0")}-${String(date.getHours()).padStart(2, "0")}${String(date.getMinutes()).padStart(2, "0")}${String(date.getSeconds()).padStart(2, "0")}`;
    downloadTextFile(`transcription-${stamp}.md`, entryToMarkdown(entry));
    setOpenMenuId(null);
    flashNotice(t("Файл сохранен"));
  }

  async function handleUseAsPrompt(entry: HistoryEntry) {
    const ok = await copyToClipboard(entry.text);
    setOpenMenuId(null);
    if (ok) flashNotice(t("Скопировано. Вставьте в LLM-обработка → Системный промпт."));
    else setError(t("Не удалось скопировать."));
  }

  async function handleCreateReplacementRule(entry: HistoryEntry) {
    const selected = window.getSelection()?.toString().trim();
    const find = selected || entry.text.slice(0, 80).trim();
    if (!find) return;
    await emit("navigate-tab", "text");
    await emit("prefill-replacement", { find, replace: "" });
    setOpenMenuId(null);
    flashNotice(t("Черновик правила открыт в разделе замен."));
  }

  async function handleClearAll() {
    if (!await confirmDestructive(t("Очистить всю историю? Это действие нельзя отменить."))) return;
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
          <div role="status" aria-live="polite" style={{ padding: "10px 12px", borderRadius: 8, background: "var(--accent-soft-2)", border: "1px solid var(--border-accent)", color: "var(--text)", font: "500 12px/1.35 var(--font-sans)" }}>{notice}</div>
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
                      onExport={() => handleExport(entry)}
                      onUseAsPrompt={() => void handleUseAsPrompt(entry)}
                      onCreateReplacementRule={() => void handleCreateReplacementRule(entry)}
                      onStartEdit={() => startEdit(entry)}
                      onCancelEdit={cancelEdit}
                      onSaveEdit={() => void saveEdit()}
                      editing={editingId === entry.id}
                      editingText={editingText}
                      onEditingTextChange={setEditingText}
                      saving={savingEditId === entry.id}
                      isProcessing={retryingId === entry.id}
                      processingAction={processingAction}
                      retryingAny={retryingId !== null}
                      currentAiConfig={currentAiConfig}
                      copiedBlockKey={copiedBlockKey}
                      onCopyBlock={handleCopyBlock}
                      expandedBlockKeys={expandedBlockKeys}
                      onToggleBlock={toggleBlock}
                      onRetryAi={() => void handleRetryAi(entry)}
                      onProcessAi={() => void handleProcessAi(entry)}
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
    <section className="card" style={{ padding: "10px 14px" }}>
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
    </section>
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
        padding: "8px 12px", borderRadius: "var(--r-sm)",
        background: "var(--accent-soft-2)", border: "1px solid var(--border-accent)",
        boxShadow: "0 2px 12px rgba(0,0,0,0.12)",
      }}
    >
      <span style={{ font: "600 12px/1 var(--font-sans)", color: "var(--text)" }}>{t("Выбрано:")} {count}</span>
      {!allVisibleSelected && totalVisible > count && (
        <button className="btn btn--ghost" onClick={onSelectAllVisible}><Icon name="check" size={11}/>{t("Выделить видимые (")}{totalVisible})</button>
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
  onExport: () => void;
  onUseAsPrompt: () => void;
  onCreateReplacementRule: () => void;
  onStartEdit: () => void;
  onCancelEdit: () => void;
  onSaveEdit: () => void;
  editing: boolean;
  editingText: string;
  onEditingTextChange: (v: string) => void;
  saving: boolean;
  isProcessing: boolean;
  processingAction: ProcessingAction | null;
  retryingAny: boolean;
  currentAiConfig: AiConfig | null;
  copiedBlockKey: string | null;
  onCopyBlock: (key: string, text: string) => void;
  expandedBlockKeys: Set<string>;
  onToggleBlock: (key: string) => void;
  onRetryAi: () => void;
  onProcessAi: () => void;
  menuOpen: boolean;
  onToggleMenu: () => void;
}) {
  const {
    entry, viewMode, selected, onToggleSelected, detailsExpanded, onToggleDetails,
    diffOn, onToggleDiff, fresh, copiedId, onCopy, onDelete, onExport, onUseAsPrompt, onCreateReplacementRule,
    onStartEdit, onCancelEdit, onSaveEdit, editing, editingText, onEditingTextChange, saving,
    isProcessing, processingAction, retryingAny, currentAiConfig,
    copiedBlockKey, onCopyBlock, expandedBlockKeys, onToggleBlock,
    onRetryAi, onProcessAi, menuOpen, onToggleMenu,
  } = props;

  const compact = viewMode === "list" && !detailsExpanded && !editing;
  const formattedKey = `${entry.id}:formatted`;
  const rawKey = `${entry.id}:raw`;
  const canRetry = canRetryAiProcessing(entry);
  const canProcess = canProcessAiProcessing(entry);
  const hasDetails = entryHasDetails(entry);
  const canDiff = !!(entry.formatted_text && entry.formatted_text !== entry.text);
  const actionTarget = aiTargetText(currentAiConfig);
  const aiBadgeColor = aiStatusColor(entry);
  const profileLabel = aiProfileLabel(entry);
  const sttLabel = transcriptionModelLabel(entry);

  const borderStyle = selected
    ? "1px solid var(--border-accent)"
    : fresh ? "1px solid var(--border-accent)" : "1px solid var(--border)";
  const cardBackground = selected
    ? "var(--accent-soft-2)"
    : fresh ? "linear-gradient(180deg, var(--accent-soft-2), var(--surface-2))" : "var(--surface-2)";

  return (
    <article
      style={{
        display: "grid",
        gridTemplateColumns: "auto 1fr auto",
        gap: 10,
        padding: compact ? "8px 10px" : 12,
        borderRadius: "var(--r-sm)",
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
          <span style={{ font: "500 11px/1 var(--font-mono)", color: "var(--text-mute)", letterSpacing: "0.04em" }}>{formatTime(entry.timestamp)}</span>
          <span style={{ font: "400 11px/1 var(--font-sans)", color: "var(--text-faint)" }}>· {relativeAge(entry.timestamp)}</span>
          <span style={{ font: "500 11px/1 var(--font-mono)", color: "var(--text-mute)" }} title={t("Модель первичной транскрибации: {p0}", { p0: sttLabel })}>
            · {t("STT: {p0}", { p0: sttLabel })}
          </span>
          {profileLabel && (
            <span style={{ font: "500 11px/1 var(--font-mono)", color: "var(--text-mute)" }} title={aiStatusText(entry)}>· {t("AI: {p0}", { p0: profileLabel })}</span>
          )}
          {fresh && <span className="tag" style={{ height: 18, fontSize: 9, background: "var(--accent-soft-2)", borderColor: "var(--border-accent)", color: "var(--text)" }}>{t("новое")}</span>}
        </div>

        {compact ? (
          <Hint text={t("Развернуть")} className="hint-anchor--block">
            <div
              onClick={onToggleDetails}
              style={{ font: "400 13px/1.4 var(--font-sans)", color: "var(--text)", display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden", cursor: "pointer" }}
            >{entry.text}</div>
          </Hint>
        ) : editing ? (
          <div style={{ display: "grid", gap: 6 }}>
            <textarea
              value={editingText}
              onChange={(e) => onEditingTextChange(e.target.value)}
              autoFocus
              rows={Math.min(12, Math.max(3, editingText.split("\n").length + 1))}
              style={{ width: "100%", padding: 8, borderRadius: "var(--r-sm)", border: "1px solid var(--border-accent)", background: "var(--surface-1)", color: "var(--text)", font: "400 13px/1.5 var(--font-sans)", resize: "vertical" }}
            />
            <div style={{ display: "flex", gap: 6 }}>
              <button className="btn btn--primary" onClick={onSaveEdit} disabled={saving}>
                {saving ? <span className="mini-spinner" aria-hidden="true"/> : <Icon name="check" size={12}/>}
                {saving ? t("Сохраняю") : t("Сохранить")}
              </button>
              <button className="btn btn--ghost" onClick={onCancelEdit} disabled={saving}><Icon name="x" size={12}/>{t("Отмена")}</button>
              <span style={{ marginLeft: "auto", alignSelf: "center", font: "400 11px/1 var(--font-mono)", color: "var(--text-mute)" }}>{editingText.length}  {t("симв.")}</span>
            </div>
          </div>
        ) : (
          <>
            <div
              title={t("{p0} симв.", { p0: entry.length })}
              style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text)", whiteSpace: "pre-wrap", overflowWrap: "break-word" }}
            >
              {entry.text}
            </div>
            {(hasDetails || canDiff) && (
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
              </div>
            )}
            {diffOn && canDiff && (
              <DiffBlock before={entry.formatted_text || ""} after={entry.text}/>
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

      <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: compact ? 0 : 140, alignItems: "stretch" }}>
        <div style={{ display: "flex", gap: 4 }}>
          <Hint text={t("Скопировать в буфер обмена")} style={{ flex: 1, minWidth: 0 }}>
          <button
            className={copiedId === entry.id ? "btn btn--primary" : "btn btn--ghost"}
            onClick={onCopy}
            aria-label={t("Скопировать")}
            style={{ width: "100%", height: 28 }}
          >
            <Icon name={copiedId === entry.id ? "check" : "copy"} size={12}/>
            {compact ? null : (copiedId === entry.id ? t("Скопировано") : t("Копировать"))}
          </button>
          </Hint>
          <ActionsMenu
            open={menuOpen}
            onToggle={onToggleMenu}
            actions={[
              { label: t("Изменить текст"), icon: "pencil", onClick: onStartEdit, disabled: editing },
              { label: t("Экспорт .md"), icon: "download", onClick: onExport },
              { label: t("Использовать как промпт"), icon: "spark", onClick: onUseAsPrompt },
              { label: t("Создать замену"), icon: "replace", onClick: onCreateReplacementRule },
              { label: t("Удалить"), icon: "trash", onClick: onDelete, danger: true },
            ]}
          />
        </div>
        {!compact && (canRetry || canProcess) && (
          <AiActionButton
            action={isProcessing && processingAction ? processingAction : canRetry ? "retry" : "process"}
            target={actionTarget}
            processing={isProcessing}
            disabled={retryingAny}
            onClick={canRetry ? onRetryAi : onProcessAi}
          />
        )}
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
      <Hint text={t("Другие действия")}>
        <button
          className="btn btn--ghost"
          onClick={onToggle}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={t("Действия")}
          style={{ height: 28, padding: "0 8px" }}
        >
          <Icon name="more" size={12}/>
        </button>
      </Hint>
      {open && (
        <div
          role="menu"
          style={{
            position: "absolute",
            right: 0,
            top: "calc(100% + 4px)",
            minWidth: 200,
            padding: 4,
            background: "var(--surface-1)",
            border: "1px solid var(--border-strong)",
            borderRadius: "var(--r-sm)",
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
                color: action.danger ? "var(--err)" : "var(--text)",
                font: "500 12px/1.1 var(--font-sans)",
                textAlign: "left",
                opacity: action.disabled ? 0.5 : 1,
              }}
              onMouseEnter={(ev) => { (ev.currentTarget as HTMLButtonElement).style.background = "var(--surface-3)"; }}
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

// Single compact LLM-action control for a history entry. The processing
// state lives INSIDE the button (spinner + "Обрабатываю…") so the card
// never grows a separate notice box, and a fixed second line under the
// button holds either the target-model caption (idle) or a thin progress
// bar (processing) — the block height is identical in both states.
function AiActionButton({ action, target, processing, disabled, onClick }: {
  action: ProcessingAction;
  target: string;
  processing: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  const isRetry = action === "retry";
  const idleLabel = isRetry ? t("Повторить LLM") : t("Обработать");
  const idleIcon = isRetry ? "wand" : "spark";
  return (
    <div style={{ display: "grid", gap: 4 }}>
      <Hint
        text={processing ? `${aiActionTitle(action)} · ${target}` : t("{p0} текст в LLM ({p1})", { p0: isRetry ? t("Повторно отправить") : t("Отправить"), p1: target })}
        className="hint-anchor--block"
      >
        <button
          className="btn btn--ghost"
          onClick={onClick}
          disabled={disabled}
          aria-busy={processing}
          aria-live="polite"
          aria-label={processing ? aiActionTitle(action) : idleLabel}
          style={{ height: 28, width: "100%" }}
        >
          {processing ? <span className="mini-spinner" aria-hidden="true"/> : <Icon name={idleIcon} size={12}/>}
          {processing ? t("Обрабатываю…") : idleLabel}
        </button>
      </Hint>
      {processing ? (
        <div aria-hidden="true" style={{ position: "relative", height: 2, borderRadius: 999, overflow: "hidden", background: "var(--surface-4)" }}>
          <div style={{ position: "absolute", inset: 0, width: "45%", borderRadius: 999, background: "var(--accent)", animation: "progress-sweep 1.3s ease-in-out infinite" }} />
        </div>
      ) : (
        <span
          title={t("Ручная обработка использует текущие настройки ИИ, а не модель, которая была активна во время записи.")}
          style={{ font: "500 9.5px/1.3 var(--font-mono)", color: "var(--text-mute)", textAlign: "center", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
        >
           {t("через")} {target}
        </span>
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
      background: "var(--surface-1)",
      borderRadius: "var(--r-sm)",
      border: "1px solid var(--border)",
      minWidth: 0,
    }}>
      <span style={{ font: "600 12.5px/1 var(--font-mono)", color: "var(--text)" }}>{value}</span>
      <span style={{ font: "500 9.5px/1 var(--font-mono)", color: "var(--text-mute)", textTransform: "uppercase", letterSpacing: "0.05em" }}>{label}</span>
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
        <div style={{ flex: "0 1 auto", minWidth: 0, font: "600 10px/1 var(--font-mono)", color: muted ? "var(--text-mute)" : "var(--text-2)", textTransform: "uppercase", letterSpacing: "0.04em" }}>{title}</div>
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
        <div style={{ font: "400 13px/1.5 var(--font-sans)", color: muted ? "var(--text-2)" : "var(--text)", whiteSpace: "pre-wrap", overflowWrap: "break-word" }}>
          {text}
        </div>
      )}
    </div>
  );
}

function EmptyState({ icon, title, hint }: { icon: string; title: string; hint: string }) {
  return (
    <div style={{ display: "grid", placeItems: "center", padding: "48px 24px", color: "var(--text-mute)", textAlign: "center" }}>
      <div style={{ marginBottom: 12, opacity: 0.7 }}><Icon name={icon} size={32}/></div>
      <div style={{ font: "500 14px/1.3 var(--font-sans)", color: "var(--text-2)" }}>{title}</div>
      {hint && <div style={{ marginTop: 6, maxWidth: 380, font: "400 12px/1.5 var(--font-sans)" }}>{hint}</div>}
    </div>
  );
}
