import type { ReplacementMatchMode, ReplacementRule } from "../bridge/types";
import { t } from "../i18n";
import { newReplacementRuleId, replacementRuleKey } from "./replacementRules";

export type ReplacementImportPolicy = "skip" | "replace";
export const MAX_REPLACEMENT_IMPORT_BYTES = 2 * 1024 * 1024;

export interface ReplacementImportRow {
  /** One-based entry number, not a JSON source line. */
  row: number;
  find: string;
  rule?: ReplacementRule;
  error?: string;
}

export interface ReplacementImportReviewRow extends ReplacementImportRow {
  status: "new" | "duplicate" | "conflict" | "invalid";
  previous?: ReplacementRule;
  /** Undefined means the matching rule is in the current list. */
  previousRow?: number;
}

const MATCH_MODES: readonly ReplacementMatchMode[] = ["word", "phrase", "contains", "regex"];
const RULE_FIELDS = new Set(["id", "find", "replace", "enabled", "match", "case_sensitive", "preserve_case", "usage_count"]);

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function parseRow(value: unknown, row: number, createId: () => string): ReplacementImportRow {
  const find = record(value) && typeof value.find === "string" ? value.find.trim() : "";
  const invalid = (error: string): ReplacementImportRow => ({ row, find, error });
  if (!record(value)) return invalid(t("Правило должно быть объектом JSON."));
  const unknown = Object.keys(value).find((key) => !RULE_FIELDS.has(key));
  if (unknown) return invalid(t("Неизвестное поле: {field}.", { field: unknown }));
  if (typeof value.find !== "string" || !find) return invalid(t("Поле find должно содержать непустую строку."));
  if (typeof value.replace !== "string") return invalid(t("Поле replace должно быть строкой; пустая строка удаляет совпадение."));
  if (value.id !== undefined && typeof value.id !== "string") return invalid(t("Поле id должно быть строкой."));
  for (const field of ["enabled", "case_sensitive", "preserve_case"] as const) {
    if (value[field] !== undefined && typeof value[field] !== "boolean") {
      return invalid(t("Поле {field} должно быть true или false.", { field }));
    }
  }
  if (value.match !== undefined && !MATCH_MODES.includes(value.match as ReplacementMatchMode)) {
    return invalid(t("Поле match: допустимы word, phrase, contains и regex."));
  }
  if (value.usage_count !== undefined && (!Number.isSafeInteger(value.usage_count) || Number(value.usage_count) < 0)) {
    return invalid(t("Поле usage_count должно быть целым неотрицательным числом."));
  }
  const mode = (value.match ?? "word") as ReplacementMatchMode;
  if (mode !== "regex" && find === value.replace) return invalid(t("Текст поиска совпадает с заменой."));
  return {
    row,
    find,
    rule: {
      // An exported id belongs to its original list. Allocate once, before review.
      id: createId(),
      find,
      replace: value.replace,
      enabled: (value.enabled ?? true) as boolean,
      match: mode,
      case_sensitive: (value.case_sensitive ?? false) as boolean,
      preserve_case: (value.preserve_case ?? false) as boolean,
      usage_count: (value.usage_count ?? 0) as number,
    },
  };
}

/** Parse the exported wrapper, a rule array, or the legacy string-to-string map.
 * Invalid rows stay in the review; the caller must refuse a partial import. */
export function parseReplacementImport(text: string, createId: () => string = newReplacementRuleId): ReplacementImportRow[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text.replace(/^\uFEFF/, ""));
  } catch {
    throw new Error(t("Не удалось прочитать JSON. Проверьте формат файла."));
  }
  let values: unknown[];
  if (Array.isArray(parsed)) {
    values = parsed;
  } else if (record(parsed) && Object.prototype.hasOwnProperty.call(parsed, "replacement_rules") && typeof parsed.replacement_rules !== "string") {
    if (!Array.isArray(parsed.replacement_rules) || Object.keys(parsed).length !== 1) {
      throw new Error(t("Ожидается объект с массивом replacement_rules без других полей."));
    }
    values = parsed.replacement_rules;
  } else if (record(parsed)) {
    values = Object.entries(parsed).map(([find, replace]) => ({ find, replace }));
  } else {
    throw new Error(t("Ожидается массив правил, объект replacement_rules или словарь замен."));
  }
  return values.map((value, index) => parseRow(value, index + 1, createId));
}

function equivalent(a: ReplacementRule, b: ReplacementRule): boolean {
  return a.replace === b.replace && a.enabled === b.enabled
    && a.case_sensitive === b.case_sensitive && a.preserve_case === b.preserve_case
    && (!(a.case_sensitive || a.match === "regex") || a.find.trim() === b.find.trim());
}

/** Linear merge: preserve current positions, append new searches in file order.
 * Replacing a match retains that list entry's identity and usage counter. */
export function reviewReplacementImport(current: ReplacementRule[], incoming: ReplacementImportRow[], policy: ReplacementImportPolicy) {
  const rules = [...current];
  const positions = new Map(current.map((rule, index) => [replacementRuleKey(rule), { index, row: undefined as number | undefined }]));
  const touched = new Set<number>();
  let duplicates = 0;
  let conflicts = 0;
  let invalid = 0;
  const rows: ReplacementImportReviewRow[] = incoming.map((entry) => {
    if (!entry.rule || entry.error) {
      invalid += 1;
      return { ...entry, status: "invalid" };
    }
    const key = replacementRuleKey(entry.rule);
    const position = positions.get(key);
    if (!position) {
      positions.set(key, { index: rules.length, row: entry.row });
      rules.push(entry.rule);
      return { ...entry, status: "new" };
    }
    const previous = rules[position.index];
    if (equivalent(previous, entry.rule)) {
      duplicates += 1;
      return { ...entry, status: "duplicate", previous, previousRow: position.row };
    }
    conflicts += 1;
    if (policy === "replace") {
      rules[position.index] = { ...entry.rule, id: previous.id, usage_count: previous.usage_count };
      if (position.index < current.length) touched.add(position.index);
      positions.set(key, { index: position.index, row: entry.row });
    }
    return { ...entry, status: "conflict", previous, previousRow: position.row };
  });
  const replaced = [...touched].filter((index) => !equivalent(current[index], rules[index])).length;
  return { rules, rows, added: rules.length - current.length, replaced, duplicates, conflicts, invalid };
}
