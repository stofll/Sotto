import { describe, expect, it } from "vitest";
import type { ReplacementRule } from "../bridge/types";
import { parseReplacementImport, reviewReplacementImport } from "./replacementImport";
import { makeReplacementRule } from "./replacementRules";

function parse(value: unknown) {
  let id = 0;
  return parseReplacementImport(JSON.stringify(value), () => `import-${++id}`);
}

function current(find: string, replace: string, extra: Partial<ReplacementRule> = {}): ReplacementRule {
  return { ...makeReplacementRule(find, replace), id: `current-${find}`, ...extra };
}

describe("replacement import parsing", () => {
  it("accepts UTF-8 BOM, trims search text and preserves an explicit empty replacement", () => {
    const rows = parseReplacementImport('\uFEFF {"replacement_rules":[{"find":"  убрать  ","replace":""}]}', () => "new");
    expect(rows).toEqual([{ row: 1, find: "убрать", rule: { id: "new", find: "убрать", replace: "", enabled: true, match: "word", case_sensitive: false, preserve_case: false, usage_count: 0 } }]);
  });

  it("accepts arrays and string maps, including keys that have a meaning on Object.prototype", () => {
    const array = parse([{ find: "sotto", replace: "Sotto" }]);
    const legacy = parse(JSON.parse('{"sotto":"Sotto","__proto__":"prototype","constructor":"builder"}'));
    expect(array[0].rule?.find).toBe("sotto");
    expect(legacy.map((row) => [row.rule?.find, row.rule?.replace])).toEqual([["sotto", "Sotto"], ["__proto__", "prototype"], ["constructor", "builder"]]);
  });

  it("allocates fresh ids instead of reusing colliding exported ids and keeps other supported settings", () => {
    const rows = parse([
      { id: "same-exported-id", find: "old", replace: "$1", match: "regex", enabled: false, case_sensitive: true, preserve_case: true, usage_count: 4 },
      { id: "same-exported-id", find: "another", replace: "new", match: "contains" },
    ]);
    expect(rows.map((row) => row.rule?.id)).toEqual(["import-1", "import-2"]);
    expect(rows[0].rule).toMatchObject({ match: "regex", enabled: false, case_sensitive: true, preserve_case: true, usage_count: 4 });
  });

  it("reports every invalid entry without string coercion or silently dropping rows", () => {
    const rows = parse([
      { find: "valid", replace: "accepted" },
      { find: 1, replace: "number" },
      { find: "missing replacement" },
      { find: "object", replace: {} },
      { find: "flag", replace: "value", enabled: "false" },
      { find: "mode", replace: "value", match: "wildcard" },
      { find: "unknown", replace: "value", typo: true },
      { find: "blank id", replace: "value", id: null },
      null,
      { find: "   ", replace: "value" },
      { find: "unchanged", replace: "unchanged" },
    ]);
    expect(rows).toHaveLength(11);
    expect(rows[0].rule).toBeDefined();
    expect(rows.slice(1).every((row) => row.error && !row.rule)).toBe(true);
    expect(rows.map((row) => row.row)).toEqual(Array.from({ length: 11 }, (_, i) => i + 1));
  });

  it.each([null, true, 4, "not a rule", { replacement_rules: null }, { replacement_rules: [], unrelated: "value" }])("rejects unsupported root shapes: %j", (value) => {
    expect(() => parse(value)).toThrow();
  });

  it("rejects malformed JSON and noninteger usage counters", () => {
    expect(() => parseReplacementImport("not json")).toThrow(/JSON/);
    for (const usage_count of [-1, 0.5, "4", Number.MAX_SAFE_INTEGER + 1]) {
      expect(parse([{ find: "count", replace: "counter", usage_count }])[0].error).toContain("usage_count");
    }
  });
});

describe("replacement import review", () => {
  it("skips duplicates and conflicts while retaining current order and appending new rules in file order", () => {
    const existing = [current("a", "A"), current("b", "B", { usage_count: 7 })];
    const incoming = parse([{ find: "a", replace: "A" }, { find: "b", replace: "changed" }, { find: "c", replace: "C" }, { find: "d", replace: "D" }]);
    const result = reviewReplacementImport(existing, incoming, "skip");
    expect(result.rules.map((rule) => [rule.find, rule.replace])).toEqual([["a", "A"], ["b", "B"], ["c", "C"], ["d", "D"]]);
    expect(result).toMatchObject({ added: 2, replaced: 0, duplicates: 1, conflicts: 1, invalid: 0 });
    expect(result.rows[1]).toMatchObject({ row: 2, status: "conflict", previous: existing[1] });
    expect(existing[1].replace).toBe("B");
  });

  it("replaces matching rows in place, retains their identity, and applies the last conflicting file entry", () => {
    const existing = [current("a", "A", { usage_count: 7 }), current("b", "B")];
    const incoming = parse([{ find: "a", replace: "first" }, { find: "c", replace: "C" }, { find: "a", replace: "last" }, { find: "c", replace: "latest" }]);
    const result = reviewReplacementImport(existing, incoming, "replace");
    expect(result.rules.map((rule) => [rule.find, rule.replace])).toEqual([["a", "last"], ["b", "B"], ["c", "latest"]]);
    expect(result.rules[0]).toMatchObject({ id: existing[0].id, usage_count: 7 });
    expect(result.rules[2].id).toBe(incoming[1].rule?.id);
    expect(result).toMatchObject({ added: 1, replaced: 1, duplicates: 0, conflicts: 3 });
    expect(result.rows[2].previousRow).toBe(1);
    expect(result.rows[3].previousRow).toBe(2);
    expect(reviewReplacementImport(existing, incoming, "skip").rules[0].replace).toBe("A");
    expect(incoming[0].rule?.id).toBe("import-1");
  });

  it("counts repeats within a file and makes first-wins versus last-wins explicit", () => {
    const rows = parse([{ find: "a", replace: "first" }, { find: "a", replace: "first" }, { find: "a", replace: "last" }]);
    const skipped = reviewReplacementImport([], rows, "skip");
    expect(skipped.rules.map((rule) => rule.replace)).toEqual(["first"]);
    expect(skipped).toMatchObject({ added: 1, duplicates: 1, conflicts: 1 });
    expect(skipped.rows[1].previousRow).toBe(1);
    expect(reviewReplacementImport([], rows, "replace").rules.map((rule) => rule.replace)).toEqual(["last"]);
  });

  it("treats flag changes and differing regex searches as conflicts rather than exact repeats", () => {
    const rules = [current("case", "value"), current("\\s", "space", { match: "regex" })];
    const rows = parse([{ find: "case", replace: "value", enabled: false }, { find: "\\S", replace: "space", match: "regex" }]);
    expect(reviewReplacementImport(rules, rows, "skip")).toMatchObject({ duplicates: 0, conflicts: 2 });
  });

  it("keeps different matching modes separate and exposes native errors by source row", () => {
    const rows = parse([{ find: "a", replace: "A", match: "phrase" }, { find: "bad regex", replace: "value", match: "regex" }]);
    rows[1].error = "Synthetic Rust regex error";
    const review = reviewReplacementImport([current("a", "A")], rows, "skip");
    expect(review).toMatchObject({ added: 1, invalid: 1, conflicts: 0 });
    expect(review.rows[1]).toMatchObject({ row: 2, status: "invalid", error: "Synthetic Rust regex error" });
  });

  it("handles a 2,000-rule round trip without duplicating entries or their ids", () => {
    const rows = parse(Array.from({ length: 2000 }, (_, index) => ({ id: "exported", find: `term-${index}`, replace: `Термин-${index}` })));
    const first = reviewReplacementImport([], rows, "skip");
    const repeated = reviewReplacementImport(first.rules, rows, "replace");
    expect(first.added).toBe(2000);
    expect(new Set(first.rules.map((rule) => rule.id)).size).toBe(2000);
    expect(repeated).toMatchObject({ added: 0, replaced: 0, duplicates: 2000 });
    expect(repeated.rules).toEqual(first.rules);
  });
});
