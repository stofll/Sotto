// Command-surface guard.
//
// The recurring class of bug this catches: the frontend calls a Tauri
// command via `invoke("name", …)` that is never registered in the Rust
// `generate_handler![…]` list, so at runtime it fails with
// "Command <name> not found". Neither the Rust unit tests (command simply
// absent) nor the Tauri-mocked frontend tests (backend stubbed) notice.
//
// This test statically extracts:
//   1. every command name the frontend invokes (invoke/rustInvoke), and
//   2. every command registered in src-tauri/src/lib.rs generate_handler!
// and asserts (1) ⊆ (2). Sources are loaded as raw strings via Vite's
// `?raw` imports (typed by vite/client) — no Node fs, no extra deps.

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

/** Command names passed as the first arg to invoke()/rustInvoke(). */
function invokedCommands(): Set<string> {
  const re = /\b(?:invoke|rustInvoke)\b[^("'`\n]*\(\s*["'`]([a-z_][a-z0-9_]*)["'`]/g;
  const names = new Set<string>();
  for (const [path, text] of Object.entries(frontendSources)) {
    if (/\.test\.(ts|tsx)$/.test(path) || path.endsWith(".d.ts")) continue;
    for (const m of text.matchAll(re)) names.add(m[1]);
  }
  return names;
}

/** Command names registered in the generate_handler![…] block. */
function registeredCommands(): Set<string> {
  const start = libRs.indexOf("generate_handler![");
  expect(start, "generate_handler! not found in lib.rs").toBeGreaterThan(-1);
  const block = libRs.slice(start, libRs.indexOf("])", start));
  const names = new Set<string>();
  for (const rawLine of block.split("\n")) {
    const line = rawLine.replace(/\/\/.*$/, ""); // strip line comments
    if (line.trim().startsWith("#")) continue; // skip #[cfg(...)] attrs
    // Take the last path segment before a comma: `format_commands::foo,` -> `foo`.
    for (const m of line.matchAll(/(?:[a-zA-Z_][a-zA-Z0-9_]*::)*([a-z_][a-z0-9_]*)\s*,/g)) {
      names.add(m[1]);
    }
  }
  return names;
}

describe("Tauri command surface", () => {
  it("registers every command the frontend invokes", () => {
    const invoked = invokedCommands();
    const registered = registeredCommands();

    // Sanity: extraction actually found the lists (guards against a regex
    // that silently matches nothing after a refactor).
    expect(invoked.size).toBeGreaterThan(10);
    expect(registered.size).toBeGreaterThan(10);
    expect(registered.has("save_config")).toBe(true);

    const missing = [...invoked].filter((name) => !registered.has(name)).sort();
    expect(missing, `Frontend invokes commands not registered in lib.rs generate_handler!: ${missing.join(", ")}`).toEqual([]);
  });
});
