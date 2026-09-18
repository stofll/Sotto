// Command-surface guard.
//
// The recurring class of bug this catches: the frontend calls a Tauri
// command via `invoke("name", …)` that is never registered in the Rust
// `generate_handler![…]` list, so at runtime it fails with
// "Command <name> not found". Neither the Rust unit tests (command simply
// absent) nor the Tauri-mocked frontend tests (backend stubbed) notice.
//
// This test statically extracts:
//   1. every command name the frontend invokes (invoke/rustInvoke/tauriInvoke), and
//   2. every command registered in src-tauri/src/lib.rs generate_handler!
// and asserts (1) ⊆ (2). Sources are loaded as raw strings via Vite's
// `?raw` imports (typed by vite/client) — no Node fs, no extra deps.
//
// A registration can also be conditional: `show_tray_popup` and
// `hide_tray_popup` sit under `#[cfg(windows)]`, so on macOS the same call
// fails with the same message. For those the check is the guard at the call
// site — the `os` reported by `get_runtime_status`, or an `isWindows`-style
// value derived from it — and a restricted command called without one fails
// here. The guard is looked for in the innermost block around the call, so a
// same-line `if (isWindows)` and a block above it both count.

import { describe, it, expect } from "vitest";
// Every frontend source as raw text (keyed by path). Test files use mock
// command names, so they're filtered out below.
const frontendSources = import.meta.glob("../**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;
// The Rust command registry.
import libRs from "../../src-tauri/src/lib.rs?raw";

/** Command names passed as the first arg to invoke()/rustInvoke()/tauriInvoke(). */
const INVOKE_CALL =
  /\b(?:invoke|rustInvoke|tauriInvoke)\b[^("'`\n]*\(\s*["'`]([a-z_][a-z0-9_]*)["'`]/g;

/** A source that only mocks the IPC layer, or declares its types. */
function isTestSource(path: string): boolean {
  return /\.test\.(ts|tsx)$/.test(path) || path.endsWith(".d.ts");
}

interface Registry {
  /** Every command in generate_handler!. */
  all: Set<string>;
  /** Command → the `#[cfg(…)]` condition it is registered under. */
  restricted: Map<string, string>;
}

/** The command names registered in the generate_handler![…] block. */
function commandRegistry(): Registry {
  const start = libRs.indexOf("generate_handler![");
  expect(start, "generate_handler! not found in lib.rs").toBeGreaterThan(-1);
  const block = libRs.slice(start, libRs.indexOf("])", start));
  const all = new Set<string>();
  const restricted = new Map<string, string>();
  let pendingCfg: string[] = [];
  for (const rawLine of block.split("\n")) {
    const line = rawLine.replace(/\/\/.*$/, ""); // strip line comments
    const cfg = line.trim().match(/^#\[cfg\((.*)\)\]$/);
    if (cfg) {
      // An attribute belongs to the entry below it and holds no names itself.
      pendingCfg.push(cfg[1].trim());
      continue;
    }
    if (line.trim().startsWith("#")) continue; // other attributes
    // Take the last path segment before a comma: `format_commands::foo,` -> `foo`.
    const names = [...line.matchAll(/(?:[a-zA-Z_][a-zA-Z0-9_]*::)*([a-z_][a-z0-9_]*)\s*,/g)].map(
      (match) => match[1],
    );
    for (const name of names) {
      all.add(name);
      if (pendingCfg.length) restricted.set(name, pendingCfg.join(" && "));
    }
    if (names.length) pendingCfg = [];
  }
  return { all, restricted };
}

/** Command names the frontend invokes. */
function invokedCommands(): Set<string> {
  const names = new Set<string>();
  for (const [path, text] of Object.entries(frontendSources)) {
    if (isTestSource(path)) continue;
    for (const match of text.matchAll(INVOKE_CALL)) names.add(match[1]);
  }
  return names;
}

/** The source with comments and string bodies blanked out, length preserved. */
function codeOnly(text: string): string {
  let out = "";
  const blank = (from: number, to: number) => {
    for (let i = from; i < to; i++) out += text[i] === "\n" ? "\n" : " ";
  };
  let i = 0;
  while (i < text.length) {
    const ch = text[i];
    if (ch === "/" && text[i + 1] === "/") {
      const end = text.indexOf("\n", i);
      const stop = end === -1 ? text.length : end;
      blank(i, stop);
      i = stop;
      continue;
    }
    if (ch === "/" && text[i + 1] === "*") {
      const end = text.indexOf("*/", i + 2);
      const stop = end === -1 ? text.length : end + 2;
      blank(i, stop);
      i = stop;
      continue;
    }
    // An apostrophe inside a word (`don't`) is text, not a quote opening.
    const inWord = ch === "'" && /\w/.test(text[i - 1] ?? "") && /\w/.test(text[i + 1] ?? "");
    if ((ch === '"' || ch === "'" || ch === "`") && !inWord) {
      let end = i + 1;
      while (end < text.length && text[end] !== ch) end += text[end] === "\\" ? 2 : 1;
      const stop = Math.min(end + 1, text.length);
      blank(i, stop);
      i = stop;
      continue;
    }
    out += ch;
    i++;
  }
  return out;
}

/** Index of the innermost bracket enclosing `at`, or 0 at the top level. */
function enclosingBracket(code: string, at: number): number {
  let depth = 0;
  for (let i = at - 1; i >= 0; i--) {
    const ch = code[i];
    if (ch === ")" || ch === "]" || ch === "}") depth++;
    else if (ch === "(" || ch === "[" || ch === "{") {
      if (depth === 0) return i;
      depth--;
    }
  }
  return 0;
}

interface InvocationSite {
  file: string;
  command: string;
  /** One-based line of the call, for the failure message. */
  line: number;
  /** Source with comments and string bodies blanked out, same offsets. */
  code: string;
  /** Character offset of the command name in `source`. */
  at: number;
}

interface FrontendSource {
  file: string;
  text: string;
  code: string;
}

function frontendCode(): FrontendSource[] {
  const sources: FrontendSource[] = [];
  for (const [file, text] of Object.entries(frontendSources)) {
    if (isTestSource(file)) continue;
    sources.push({ file, text, code: codeOnly(text) });
  }
  return sources;
}

function invocationSites(sources: FrontendSource[]): InvocationSite[] {
  const sites: InvocationSite[] = [];
  for (const { file, text, code } of sources) {
    for (const match of text.matchAll(INVOKE_CALL)) {
      const at = match.index ?? 0;
      sites.push({
        file,
        command: match[1],
        line: text.slice(0, at).split("\n").length,
        code,
        at,
      });
    }
  }
  return sites;
}

/** A platform predicate: an `isWindows`-style value or a comparison with `os`. */
const PLATFORM_GUARD =
  /\bis(Windows|MacOS|Mac|Linux)(?:Os)?\b|\bos\s*===?\s*["'`](windows|macos|linux)["'`]/gi;

/** Whether `text` names the platform that `cfg` restricts the command to. */
function hasPlatformGuard(text: string, cfg: string): boolean {
  const found = new Set<string>();
  for (const match of text.matchAll(PLATFORM_GUARD)) {
    const named = (match[1] ?? match[2] ?? "").toLowerCase();
    if (named.startsWith("windows")) found.add("windows");
    else if (named.startsWith("mac")) found.add("macos");
    else if (named.startsWith("linux")) found.add("linux");
  }
  // `#[cfg(windows)]` asks for the Windows predicate; a condition this parser
  // does not resolve (`all(…)`, a feature flag) accepts any platform guard.
  const required = cfg.match(/\b(?:windows|macos|linux)\b/g);
  if (!required) return found.size > 0;
  return required.some((name) => found.has(name.toLowerCase()));
}

/** Whether the bracket at `opener` starts a function or arrow body. */
function opensFunctionBody(code: string, opener: number): boolean {
  const lineStart = code.lastIndexOf("\n", opener) + 1;
  return /\bfunction\b|=>/.test(code.slice(lineStart, opener));
}

/** Offset just after the `;`, `{` or `}` that ends the previous statement. */
function statementStart(text: string, at: number): number {
  const before = text.slice(0, at);
  return Math.max(before.lastIndexOf(";"), before.lastIndexOf("{"), before.lastIndexOf("}")) + 1;
}

/**
 * Whether the call sits behind a platform guard. The guard counts when it is
 * on the call's own line (`if (isWindows) await hide_tray_popup()`), on the
 * line that opens the block around the call (`if (isWindows) {`), or in an
 * enclosing expression — the JSX `{isWindows && …}` around the button. Inside
 * a function only the call's own statement counts, so an `isWindows` declared
 * earlier in the component body does not stand in for a guard.
 *
 * Every lookup reads `source.code`, never `source.text`: only code can guard a
 * call. Matching the raw text instead let a comment that merely mentions
 * `isWindows` — or a string containing the word — stand in for the branch that
 * was supposed to be there, which is the one mistake this check exists to
 * catch. `codeOnly` preserves offsets, so the two are interchangeable as
 * positions and differ only in what they still contain.
 */
function callIsGuarded(source: FrontendSource, at: number, cfg: string): boolean {
  const lines = source.code.split("\n");
  const lineAt = (index: number) => source.code.slice(0, index).split("\n").length - 1;
  if (hasPlatformGuard(lines[lineAt(at)] ?? "", cfg)) return true;

  let cursor = at;
  // The levels are few; the bound only keeps a malformed file from looping.
  for (let level = 0; level < 8; level++) {
    const opener = enclosingBracket(source.code, cursor);
    if (opener === 0 || opener >= cursor) break;
    if (hasPlatformGuard(lines[lineAt(opener)] ?? "", cfg)) return true;
    const body = opensFunctionBody(source.code, opener);
    const from = body ? Math.max(opener, statementStart(source.code, cursor)) : opener;
    if (hasPlatformGuard(source.code.slice(from, cursor), cfg)) return true;
    if (body) break;
    cursor = opener;
  }
  return false;
}

describe("Tauri command surface", () => {
  const { all, restricted } = commandRegistry();

  it("registers every command the frontend invokes", () => {
    const invoked = invokedCommands();

    // Sanity: extraction actually found the lists (guards against a regex
    // that silently matches nothing after a refactor).
    expect(invoked.size).toBeGreaterThan(10);
    expect(all.size).toBeGreaterThan(10);
    expect(all.has("save_config")).toBe(true);

    const missing = [...invoked].filter((name) => !all.has(name)).sort();
    expect(missing, `Frontend invokes commands not registered in lib.rs generate_handler!: ${missing.join(", ")}`).toEqual([]);
  });

  it("reads the platform condition off a registration", () => {
    // The tray's command is the one the guard test below depends on: a parser
    // that stopped seeing `#[cfg(windows)]` would let that test pass for the
    // wrong reason, so the condition is pinned here.
    expect(restricted.get("hide_tray_popup")).toBe("windows");
    expect(restricted.get("show_tray_popup")).toBe("windows");
    expect(restricted.has("save_config")).toBe(false);
  });

  it("guards every call to a platform-restricted command", () => {
    const sources = frontendCode();
    const calls = invocationSites(sources).filter((site) => restricted.has(site.command));
    // Anti-vacuity: the frontend does call a restricted command today, and a
    // refactor that removed the call should revisit this check, not skip it.
    expect(calls.length).toBeGreaterThan(0);

    const unguarded = calls
      .filter((site) => {
        const source = sources.find((candidate) => candidate.file === site.file)!;
        return !callIsGuarded(source, site.at, restricted.get(site.command)!);
      })
      .map((site) => `${site.file}:${site.line}: ${site.command} is registered under #[cfg(${restricted.get(site.command)})] but the call has no platform guard`);
    expect(unguarded, unguarded.join("\n")).toEqual([]);
  });

  // A guard is a branch, not a mention. Without this the check above degrades
  // into a spell-check that any comment naming the platform can satisfy — and
  // it did, until `callIsGuarded` was pointed at the stripped source.
  it("reads a guard out of code, not out of comments or strings", () => {
    const guarded = (body: string) => {
      const text = `async function openMain() {\n${body}\n}\n`;
      const at = [...text.matchAll(INVOKE_CALL)][0]?.index;
      expect(at, "the fixture must contain a call to match").toBeDefined();
      return callIsGuarded({ file: "fixture.tsx", text, code: codeOnly(text) }, at!, "windows");
    };

    expect(guarded('  if (isWindows) await invoke("hide_tray_popup");')).toBe(true);
    expect(guarded('  if (isWindows) {\n    await invoke("hide_tray_popup");\n  }')).toBe(true);

    expect(guarded('  await invoke("hide_tray_popup");')).toBe(false);
    expect(guarded('  // isWindows: the popup is Windows-only\n  await invoke("hide_tray_popup");')).toBe(false);
    expect(guarded('  report("isWindows");\n  await invoke("hide_tray_popup");')).toBe(false);
    // Declared earlier in the same body, but not on the path to the call.
    expect(guarded('  const ready = isWindows;\n  await invoke("hide_tray_popup");')).toBe(false);
  });
});
