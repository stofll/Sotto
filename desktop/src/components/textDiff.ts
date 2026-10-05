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

// Token edits (a word with added punctuation costs two: remove and add) that
// the exact alignment may find before giving up. Myers' search costs time
// proportional to text length times edits, and its trace about edits squared,
// so a tidy-up of an hour-long transcript stays cheap while a wholesale
// rewrite gives up within ~40 ms and ~16 MiB, rendering whole removed/added
// runs instead of freezing the UI.
const MAX_EDIT_DISTANCE = 2_000;

/** Split into words *and* the whitespace between them, so the runs can be
 *  reassembled without inventing separators. */
function splitWords(text: string): string[] {
  return text.split(/(\s+)/);
}

/** Token diff around the shared head and tail. Emits one segment per token;
 *  callers merge. */
function diffTokens(a: string[], b: string[]): DiffSegment[] {
  const prefix = commonPrefixLength(a, b);
  const suffix = commonSuffixLength(a, b, prefix);
  const middleA = a.slice(prefix, a.length - suffix);
  const middleB = b.slice(prefix, b.length - suffix);
  const middle = middleA.length && middleB.length ? alignTokens(middleA, middleB) : null;
  return [
    { text: a.slice(0, prefix).join(""), change: "keep" },
    ...(middle ?? [
      { text: middleA.join(""), change: "remove" },
      { text: middleB.join(""), change: "add" },
    ]),
    { text: a.slice(a.length - suffix).join(""), change: "keep" },
  ];
}

/**
 * Shortest edit script by Myers' O((N+M)·D) algorithm, or `null` past
 * {@link MAX_EDIT_DISTANCE}.
 *
 * Within each run of changes the deletions come first, which is what makes a
 * replacement an adjacent remove/add pair that {@link refineReplacement} can
 * then look at.
 */
function alignTokens(a: string[], b: string[]): DiffSegment[] | null {
  const n = a.length;
  const m = b.length;
  const maxD = Math.min(n + m, MAX_EDIT_DISTANCE);
  // Furthest x reached on each diagonal k = x - y, indexed from -maxD - 1.
  const offset = maxD + 1;
  const v = new Int32Array(2 * maxD + 3);
  // Snapshot of diagonals -d..d after each step, for the walk back.
  const trace: Int32Array[] = [];
  let distance = -1;
  for (let d = 0; d <= maxD && distance < 0; d++) {
    for (let k = -d; k <= d; k += 2) {
      let x = k === -d || (k !== d && v[offset + k - 1] < v[offset + k + 1])
        ? v[offset + k + 1]
        : v[offset + k - 1] + 1;
      let y = x - k;
      while (x < n && y < m && a[x] === b[y]) { x++; y++; }
      v[offset + k] = x;
      if (x >= n && y >= m) distance = d;
    }
    trace.push(v.slice(offset - d, offset + d + 1));
  }
  if (distance < 0) return null;

  const reversed: DiffSegment[] = [];
  let x = n;
  let y = m;
  for (let d = distance; d > 0; d--) {
    const previous = trace[d - 1];
    const k = x - y;
    const inserted = k === -d || (k !== d && previous[k - 1 + d - 1] < previous[k + 1 + d - 1]);
    const previousK = inserted ? k + 1 : k - 1;
    const startX = previous[previousK + d - 1] + (inserted ? 0 : 1);
    while (x > startX) { reversed.push({ text: a[--x], change: "keep" }); y--; }
    if (inserted) reversed.push({ text: b[--y], change: "add" });
    else reversed.push({ text: a[--x], change: "remove" });
  }
  while (x > 0) reversed.push({ text: a[--x], change: "keep" });
  return removalsFirst(reversed.reverse());
}

/** Reorder each run of changes so its deletions precede its insertions. Both
 *  sides still read back in order, since only remove/add pairs swap. */
function removalsFirst(segments: DiffSegment[]): DiffSegment[] {
  const out: DiffSegment[] = [];
  let added: DiffSegment[] = [];
  for (const segment of segments) {
    if (segment.change === "add") { added.push(segment); continue; }
    if (segment.change === "keep") { out.push(...added); added = []; }
    out.push(segment);
  }
  out.push(...added);
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
