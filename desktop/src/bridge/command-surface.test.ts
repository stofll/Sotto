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
// here. The guard is a TypeScript condition that implies the platform, so a
// same-line `if (isWindows)` and a block above it both count, while a mention
// of `isWindows` in a comment, a negation, or a `||` does not.

import { describe, it, expect } from "vitest";
import ts from "typescript";
// Every frontend source as raw text (keyed by path). Test files use mock
// command names, so they're filtered out below.
const frontendSources = import.meta.glob("../**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;
// The Rust command registry.
import libRs from "../../src-tauri/src/lib.rs?raw";
import setupRs from "../../setup/src-tauri/src/main.rs?raw";

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
function commandRegistry(source = libRs): Registry {
  const start = source.indexOf("generate_handler![");
  expect(start, "generate_handler! not found in lib.rs").toBeGreaterThan(-1);
  const block = source.slice(start, source.indexOf("])", start));
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
function isSetupSource(path: string): boolean {
  return path.endsWith("/installer.ts") || path.startsWith("../installer/");
}

function invokedCommands(setup = false): Set<string> {
  const names = new Set<string>();
  for (const [path, text] of Object.entries(frontendSources)) {
    if (isTestSource(path) || isSetupSource(path) !== setup) continue;
    for (const match of text.matchAll(INVOKE_CALL)) names.add(match[1]);
  }
  return names;
}

interface InvocationSite {
  file: string;
  command: string;
  /** One-based line of the call, for the failure message. */
  line: number;
  /** Character offset of the command name in `source`. */
  at: number;
}

interface FrontendSource {
  file: string;
  text: string;
}

function frontendCode(): FrontendSource[] {
  const sources: FrontendSource[] = [];
  for (const [file, text] of Object.entries(frontendSources)) {
    if (isTestSource(file) || isSetupSource(file)) continue;
    sources.push({ file, text });
  }
  return sources;
}

function invocationSites(sources: FrontendSource[]): InvocationSite[] {
  const sites: InvocationSite[] = [];
  for (const { file, text } of sources) {
    for (const match of text.matchAll(INVOKE_CALL)) {
      const at = match.index ?? 0;
      sites.push({
        file,
        command: match[1],
        line: text.slice(0, at).split("\n").length,
        at,
      });
    }
  }
  return sites;
}

function parseSource(file: string, text: string): ts.SourceFile {
  const kind = file.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  return ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, kind);
}

function unwrap(expr: ts.Expression): ts.Expression {
  while (ts.isParenthesizedExpression(expr) || ts.isAsExpression(expr) || ts.isNonNullExpression(expr)) {
    expr = expr.expression;
  }
  return expr;
}

function platformFromIsName(name: string): string | undefined {
  const match = /^is(Windows|MacOS|Mac|Linux)(?:Os)?$/i.exec(name);
  if (!match) return undefined;
  const named = match[1].toLowerCase();
  if (named.startsWith("windows")) return "windows";
  if (named.startsWith("mac")) return "macos";
  return "linux";
}

function isOsAccess(expr: ts.Expression): boolean {
  if (ts.isIdentifier(expr)) return expr.text === "os";
  return ts.isPropertyAccessExpression(expr) && expr.name.text === "os";
}

function platformLiteral(expr: ts.Expression): string | undefined {
  if (!ts.isStringLiteralLike(expr)) return undefined;
  const value = expr.text.toLowerCase();
  if (value === "windows" || value === "macos" || value === "linux") return value;
  return undefined;
}

function platformFromEquality(expr: ts.BinaryExpression): string | undefined {
  const left = unwrap(expr.left);
  const right = unwrap(expr.right);
  if (isOsAccess(left)) return platformLiteral(right);
  if (isOsAccess(right)) return platformLiteral(left);
  return undefined;
}

/** Platforms that must hold when `expr` is true. `a && b` unions, `a || b` intersects. */
function platformsImplied(expr: ts.Expression): Set<string> {
  expr = unwrap(expr);
  if (ts.isPrefixUnaryExpression(expr) && expr.operator === ts.SyntaxKind.ExclamationToken) {
    return new Set();
  }
  if (ts.isBinaryExpression(expr)) {
    const op = expr.operatorToken.kind;
    if (op === ts.SyntaxKind.AmpersandAmpersandToken) {
      return new Set([...platformsImplied(expr.left), ...platformsImplied(expr.right)]);
    }
    if (op === ts.SyntaxKind.BarBarToken) {
      const left = platformsImplied(expr.left);
      const right = platformsImplied(expr.right);
      return new Set([...left].filter((name) => right.has(name)));
    }
    if (op === ts.SyntaxKind.EqualsEqualsEqualsToken || op === ts.SyntaxKind.EqualsEqualsToken) {
      const named = platformFromEquality(expr);
      return named ? new Set([named]) : new Set();
    }
    return new Set();
  }
  if (ts.isIdentifier(expr)) {
    const named = platformFromIsName(expr.text);
    return named ? new Set([named]) : new Set();
  }
  if (ts.isCallExpression(expr) && ts.isIdentifier(expr.expression)) {
    const named = platformFromIsName(expr.expression.text);
    return named ? new Set([named]) : new Set();
  }
  return new Set();
}

function exprImpliesCfg(expr: ts.Expression, cfg: string): boolean {
  const implied = platformsImplied(expr);
  const required = cfg.match(/\b(?:windows|macos|linux)\b/g);
  if (!required) return implied.size > 0;
  return required.some((name) => implied.has(name.toLowerCase()));
}

function isNegationOfCfg(expr: ts.Expression, cfg: string): boolean {
  const inner = unwrap(expr);
  return (
    ts.isPrefixUnaryExpression(inner) &&
    inner.operator === ts.SyntaxKind.ExclamationToken &&
    exprImpliesCfg(inner.operand, cfg)
  );
}

function nodeContains(container: ts.Node, node: ts.Node): boolean {
  for (let current: ts.Node | undefined = node; current; current = current.parent) {
    if (current === container) return true;
  }
  return false;
}

function isFunctionLike(node: ts.Node): boolean {
  return (
    ts.isFunctionDeclaration(node) ||
    ts.isFunctionExpression(node) ||
    ts.isArrowFunction(node) ||
    ts.isMethodDeclaration(node) ||
    ts.isConstructorDeclaration(node)
  );
}

/** An `onClick={() => invoke()}` handler only exists if the element is rendered. */
function isJsxAttributeHandler(fn: ts.Node): boolean {
  const parent = fn.parent;
  return Boolean(parent && ts.isJsxExpression(parent) && parent.parent && ts.isJsxAttribute(parent.parent));
}

function deepestNodeAt(sourceFile: ts.SourceFile, pos: number): ts.Node {
  let best: ts.Node = sourceFile;
  const visit = (node: ts.Node) => {
    if (pos >= node.getStart(sourceFile) && pos < node.end) {
      best = node;
      node.forEachChild(visit);
    }
  };
  visit(sourceFile);
  return best;
}

function enclosingCall(node: ts.Node): ts.CallExpression | undefined {
  for (let current: ts.Node | undefined = node; current; current = current.parent) {
    if (ts.isCallExpression(current)) return current;
  }
  return undefined;
}

/**
 * Whether the call sits behind a platform guard. A guard is a condition that
 * *implies* the platform: `if (isWindows)`, `isWindows && …`, `os === "windows"`.
 * A mention is not a guard — `if (!isWindows)`, `isWindows || enabled`, and
 * `const ready = isWindows` on the same line as the call all fail.
 *
 * JSX event handlers are walked through so `{isWindows && <button onClick={…} />}`
 * counts; a function body otherwise stops the walk, so an `isWindows` declared
 * earlier in the component does not stand in for a branch. Comments and
 * unrelated string literals never participate: the tree is parsed from source,
 * not from a stripped copy that would also erase `os === "windows"`.
 */
function callIsGuarded(source: FrontendSource, at: number, cfg: string): boolean {
  const sourceFile = parseSource(source.file, source.text);
  const call = enclosingCall(deepestNodeAt(sourceFile, at));
  if (!call) return false;

  for (let current: ts.Node | undefined = call.parent; current; current = current.parent) {
    if (ts.isIfStatement(current)) {
      if (nodeContains(current.thenStatement, call) && exprImpliesCfg(current.expression, cfg)) return true;
      if (
        current.elseStatement &&
        nodeContains(current.elseStatement, call) &&
        isNegationOfCfg(current.expression, cfg)
      ) {
        return true;
      }
    }
    if (ts.isBinaryExpression(current) && current.operatorToken.kind === ts.SyntaxKind.AmpersandAmpersandToken) {
      if (nodeContains(current.right, call) && exprImpliesCfg(current.left, cfg)) return true;
    }
    if (ts.isConditionalExpression(current)) {
      if (nodeContains(current.whenTrue, call) && exprImpliesCfg(current.condition, cfg)) return true;
      if (nodeContains(current.whenFalse, call) && isNegationOfCfg(current.condition, cfg)) return true;
    }
    if (isFunctionLike(current) && !isJsxAttributeHandler(current)) break;
  }
  return false;
}

describe("Tauri command surface", () => {
  const { all, restricted } = commandRegistry();

  it("keeps installer commands in the separate setup runtime", () => {
    const setupCommands = commandRegistry(setupRs).all;
    const invoked = invokedCommands(true);
    expect(invoked.size).toBe(6);
    expect([...invoked].filter((name) => !setupCommands.has(name))).toEqual([]);
    expect([...invoked].filter((name) => all.has(name))).toEqual([]);
  });

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

  // A guard is a branch that implies the platform, not a mention of its name.
  it("reads a guard out of a condition, not out of a mention", () => {
    const guarded = (body: string) => {
      const text = body.includes("<")
        ? `function Tray() {\n  return <div>${body}</div>;\n}\n`
        : `async function openMain() {\n${body}\n}\n`;
      const at = [...text.matchAll(INVOKE_CALL)][0]?.index;
      expect(at, "the fixture must contain a call to match").toBeDefined();
      return callIsGuarded({ file: "fixture.tsx", text }, at!, "windows");
    };

    expect(guarded('  if (isWindows) await invoke("hide_tray_popup");')).toBe(true);
    expect(guarded('  if (isWindows) {\n    await invoke("hide_tray_popup");\n  }')).toBe(true);
    expect(guarded('  isWindows && await invoke("hide_tray_popup");')).toBe(true);
    expect(guarded('  if (isWindows && enabled) await invoke("hide_tray_popup");')).toBe(true);
    expect(guarded('  if (os === "windows") await invoke("hide_tray_popup");')).toBe(true);
    expect(guarded('  if (runtime.os === "windows") await invoke("hide_tray_popup");')).toBe(true);
    expect(guarded('{isWindows && <button onClick={() => invoke("hide_tray_popup")} />}')).toBe(true);

    expect(guarded('  await invoke("hide_tray_popup");')).toBe(false);
    expect(guarded('  // isWindows: the popup is Windows-only\n  await invoke("hide_tray_popup");')).toBe(false);
    expect(guarded('  report("isWindows");\n  await invoke("hide_tray_popup");')).toBe(false);
    // Declared earlier in the same body, but not on the path to the call.
    expect(guarded('  const ready = isWindows;\n  await invoke("hide_tray_popup");')).toBe(false);
    expect(guarded('  const ready = isWindows; await invoke("hide_tray_popup");')).toBe(false);
    expect(guarded('  if (!isWindows) await invoke("hide_tray_popup");')).toBe(false);
    expect(guarded('  if (!isWindows) {\n    await invoke("hide_tray_popup");\n  }')).toBe(false);
    expect(guarded('  if (isWindows || enabled) await invoke("hide_tray_popup");')).toBe(false);
    expect(guarded('  !isWindows && await invoke("hide_tray_popup");')).toBe(false);
    expect(guarded('  if (os === "macos") await invoke("hide_tray_popup");')).toBe(false);
  });
});
