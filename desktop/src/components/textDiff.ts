/**
 * Diff for the history / preview blocks.
 *
 * Word-level alignment, then a character-level pass over each replaced run.
 * The second pass is the point: without it a comma appended to a word makes
 * the whole word read as "deleted, then reinserted", and the eye has to
 * compare two identical-looking words to find the one character that moved.
 */

export type DiffSegment = { text: string; change: "keep" | "add" | "remove" };

/**
 * Minimum share of the shorter side that must survive as a common prefix or
 * suffix before a replacement is refined.
 *
 * Below it the two runs are different words rather than two spellings of the
 * same one, and splitting them into shared letters produces confetti: for
 * «материал» → «модель» the only thing in common is the leading «м», and
 * highlighting the rest letter by letter is harder to read than replacing the
 * word whole.
 */
const MIN_COMMON_SHARE = 0.34;

/** Split into words *and* the whitespace between them, so the runs can be
 *  reassembled without inventing separators. */
function splitWords(text: string): string[] {
  return text.split(/(\s+)/);
}

/**
 * LCS diff over tokens. Emits one segment per token; callers merge.
 *
 * The tie-break (`>=`) puts deletions before insertions, which is what makes
 * a replacement come out as an adjacent remove/add pair that
 * {@link refineReplacement} can then look at.
 */
function diffTokens(a: string[], b: string[]): DiffSegment[] {
  const m = a.length;
  const n = b.length;
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));
  for (let i = m - 1; i >= 0; i--) {
    for (let j = n - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const out: DiffSegment[] = [];
  let i = 0;
  let j = 0;
  while (i < m && j < n) {
    if (a[i] === b[j]) { out.push({ text: a[i], change: "keep" }); i++; j++; }
    else if (dp[i + 1][j] >= dp[i][j + 1]) { out.push({ text: a[i], change: "remove" }); i++; }
    else { out.push({ text: b[j], change: "add" }); j++; }
  }
  while (i < m) out.push({ text: a[i++], change: "remove" });
  while (j < n) out.push({ text: b[j++], change: "add" });
  return out;
}

function mergeAdjacent(segments: DiffSegment[]): DiffSegment[] {
  const merged: DiffSegment[] = [];
  for (const segment of segments) {
    if (!segment.text) continue;
    const last = merged[merged.length - 1];
    if (last && last.change === segment.change) last.text += segment.text;
    else merged.push({ ...segment });
  }
  return merged;
}

/** Code points, not UTF-16 units: slicing mid-surrogate would corrupt the
 *  text being displayed. */
function codePoints(text: string): string[] {
  return Array.from(text);
}

function commonPrefixLength(a: string[], b: string[]): number {
  const limit = Math.min(a.length, b.length);
  let i = 0;
  while (i < limit && a[i] === b[i]) i++;
  return i;
}

function commonSuffixLength(a: string[], b: string[], skip: number): number {
  const limit = Math.min(a.length, b.length) - skip;
  let i = 0;
  while (i < limit && a[a.length - 1 - i] === b[b.length - 1 - i]) i++;
  return i;
}

/**
 * Narrow a replaced run down to the part that actually changed.
 *
 * Only the shared head and tail are peeled off — deliberately, rather than
 * running a second LCS over the characters. A character LCS finds letters in
 * common anywhere, which for two unrelated words means a scattered mess of
 * green and red; a shared prefix/suffix is what an edit actually looks like
 * (added punctuation, changed case, a different ending).
 *
 * Returns the untouched remove/add pair when the two runs have too little in
 * common to be spellings of the same thing.
 */
function refineReplacement(removed: string, added: string): DiffSegment[] {
  const coarse: DiffSegment[] = [
    { text: removed, change: "remove" },
    { text: added, change: "add" },
  ];
  const a = codePoints(removed);
  const b = codePoints(added);
  const prefix = commonPrefixLength(a, b);
  const suffix = commonSuffixLength(a, b, prefix);
  const shared = prefix + suffix;
  if (shared === 0) return coarse;
  if (shared < Math.min(a.length, b.length) * MIN_COMMON_SHARE) return coarse;

  return [
    { text: a.slice(0, prefix).join(""), change: "keep" },
    { text: a.slice(prefix, a.length - suffix).join(""), change: "remove" },
    { text: b.slice(prefix, b.length - suffix).join(""), change: "add" },
    { text: a.slice(a.length - suffix).join(""), change: "keep" },
  ];
}

/** Rewrite every adjacent remove→add pair through {@link refineReplacement}. */
function refineReplacements(runs: DiffSegment[]): DiffSegment[] {
  const out: DiffSegment[] = [];
  for (let i = 0; i < runs.length; i++) {
    const current = runs[i];
    const next = runs[i + 1];
    if (current.change === "remove" && next?.change === "add") {
      out.push(...refineReplacement(current.text, next.text));
      i++;
      continue;
    }
    out.push(current);
  }
  return out;
}

export function wordDiff(before: string, after: string): DiffSegment[] {
  const runs = mergeAdjacent(diffTokens(splitWords(before), splitWords(after)));
  return mergeAdjacent(refineReplacements(runs));
}
