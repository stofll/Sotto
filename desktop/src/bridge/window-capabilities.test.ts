// Window capability guard.
//
// `build.rs` declares every application command in Tauri's app manifest, so
// a window may call only the commands its capability grants. This test keeps
// three lists in step:
//   1. the commands registered in `generate_handler!`,
//   2. the commands declared in `build.rs`,
//   3. per window, the commands its capabilities grant, which must be exactly
//      the commands its entry point can reach through static and dynamic
//      imports — a missing grant fails at runtime with "not allowed by ACL",
//      an extra one is privilege the window does not need.

import { describe, expect, it } from "vitest";

const frontendSources = import.meta.glob("../**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;
const capabilityFiles = import.meta.glob("../../src-tauri/capabilities/*.json", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;
import libRs from "../../src-tauri/src/lib.rs?raw";
import buildRs from "../../src-tauri/build.rs?raw";

/** Window label → the module its HTML page loads, relative to `src/`. */
const WINDOW_ENTRIES: Record<string, string> = {
  main: "main.tsx",
  overlay: "overlay/main.tsx",
  "tray-popup": "tray/main.tsx",
};

const INVOKE_CALL =
  /\b(?:invoke|rustInvoke|tauriInvoke)\b[^("'`\n]*\(\s*["'`]([a-z_][a-z0-9_]*)["'`]/g;
const IMPORT_SPECIFIER =
  /\bfrom\s*["']([^"']+)["']|\bimport\s*\(\s*["']([^"']+)["']\s*\)|\bimport\s+["']([^"']+)["']/g;

/** Sources keyed by path relative to `src/`, without test files. The glob
 * keys are relative to this file: `./x.ts` here in `bridge/`, `../y/z.ts`
 * elsewhere. */
const sources = new Map(
  Object.entries(frontendSources)
    .filter(([path]) => !/\.test\.tsx?$/.test(path) && !path.endsWith(".d.ts"))
    .map(([path, text]) => [
      path.startsWith("./") ? `bridge/${path.slice(2)}` : path.replace(/^\.\.\//, ""),
      text,
    ]),
);

function normalize(path: string): string | undefined {
  const parts: string[] = [];
  for (const part of path.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (!parts.length) return undefined; // outside src/
      parts.pop();
    } else {
      parts.push(part);
    }
  }
  return parts.join("/");
}

function resolveImport(from: string, specifier: string): string | undefined {
  if (!specifier.startsWith(".")) return undefined;
  const base = normalize(`${from.split("/").slice(0, -1).join("/")}/${specifier}`);
  if (base === undefined) return undefined;
  return [base, `${base}.ts`, `${base}.tsx`, `${base}/index.ts`, `${base}/index.tsx`].find(
    (candidate) => sources.has(candidate),
  );
}

function reachableModules(entry: string): Set<string> {
  const seen = new Set<string>();
  const pending = [entry];
  while (pending.length) {
    const module = pending.pop()!;
    if (seen.has(module)) continue;
    const text = sources.get(module);
    expect(text, `module ${module} not found`).toBeDefined();
    seen.add(module);
    for (const match of text!.matchAll(IMPORT_SPECIFIER)) {
      const target = resolveImport(module, match[1] ?? match[2] ?? match[3]);
      if (target) pending.push(target);
    }
  }
  return seen;
}

function invokedFrom(entry: string): Set<string> {
  const commands = new Set<string>();
  for (const module of reachableModules(entry)) {
    for (const match of sources.get(module)!.matchAll(INVOKE_CALL)) commands.add(match[1]);
  }
  return commands;
}

/** Every command name in `generate_handler![…]`, whatever its `#[cfg]`. */
function registeredCommands(): Set<string> {
  const start = libRs.indexOf("generate_handler![");
  const block = libRs.slice(start, libRs.indexOf("])", start)).replace(/\/\/.*$/gm, "");
  return new Set([...block.matchAll(/(?:\w+::)*([a-z_][a-z0-9_]*)\s*,/g)].map((m) => m[1]));
}

function manifestCommands(): Set<string> {
  const start = buildRs.indexOf("APP_COMMANDS");
  expect(start, "APP_COMMANDS not found in build.rs").toBeGreaterThan(-1);
  const block = buildRs.slice(start, buildRs.indexOf("];", start));
  return new Set([...block.matchAll(/"([a-z_][a-z0-9_]*)"/g)].map((m) => m[1]));
}

/** Window label → application commands its capabilities allow. */
function grantedCommands(): Map<string, Set<string>> {
  const granted = new Map<string, Set<string>>();
  for (const [file, raw] of Object.entries(capabilityFiles)) {
    const capability = JSON.parse(raw) as { windows?: string[]; permissions?: unknown[] };
    const commands = (capability.permissions ?? [])
      .map((permission) => (typeof permission === "string" ? permission : ""))
      .filter((permission) => /^allow-[a-z0-9-]+$/.test(permission))
      .map((permission) => permission.slice("allow-".length).replace(/-/g, "_"));
    expect(capability.windows?.length, `${file} names no window`).toBeTruthy();
    for (const window of capability.windows ?? []) {
      const set = granted.get(window) ?? new Set<string>();
      commands.forEach((command) => set.add(command));
      granted.set(window, set);
    }
  }
  return granted;
}

const sorted = (set: Set<string>) => [...set].sort();

describe("window capabilities", () => {
  it("declares every registered command in the app manifest", () => {
    expect(sorted(manifestCommands())).toEqual(sorted(registeredCommands()));
  });

  it.each(Object.entries(WINDOW_ENTRIES))(
    "grants the %s window exactly the commands its code invokes",
    (window, entry) => {
      const invoked = invokedFrom(entry);
      expect(invoked.size, `${window} invokes no command`).toBeGreaterThan(0);
      expect(sorted(grantedCommands().get(window) ?? new Set())).toEqual(sorted(invoked));
    },
  );

  it("follows static, re-exported and dynamic imports", () => {
    // The settings window loads its pages lazily; a walk that missed dynamic
    // imports would report far fewer commands for it than the other checks.
    const main = reachableModules(WINDOW_ENTRIES.main);
    expect(main.has("pages/HistoryPage.tsx")).toBe(true);
    expect(main.has("bridge/invoke.ts")).toBe(true);
    expect(reachableModules(WINDOW_ENTRIES.overlay).has("pages/HistoryPage.tsx")).toBe(false);
  });
});
