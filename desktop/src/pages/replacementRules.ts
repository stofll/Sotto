import type { ReplacementRule } from "../bridge/types";

export function newReplacementRuleId(): string {
  return typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `rule-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export function makeReplacementRule(find = "", replace = ""): ReplacementRule {
  return { id: newReplacementRuleId(), find, replace, enabled: true, match: "word", case_sensitive: false, preserve_case: false, usage_count: 0 };
}

/** Keep the editor's existing collision semantics for search text and mode. */
export function replacementRuleKey(rule: ReplacementRule): string {
  return `${rule.match}:${rule.find.trim().toLowerCase()}`;
}
