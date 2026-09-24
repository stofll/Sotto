// Argument-name guard for Tauri commands.
//
// Sibling of `command-surface.test.ts`, which checks that every command the
// frontend calls exists. This one checks that the arguments it passes exist
// too — the failure mode is quieter and therefore worse: Tauri silently
// ignores a key the command does not declare, and leaves a missing one as
// `None`. Nothing throws. The feature just does not do what it says, and
// only at runtime, only on that one code path.
//
// Two real defects this found on its first run:
//   • three call sites passed `provider:` to `save_api_key`, which stopped
//     accepting it at some point and never told anyone;
//   • the casing rule is not decorative — Tauri maps a Rust `session_id`
//     to `sessionId` in JS unless the command opts into
//     `rename_all = "snake_case"`, so a command and its sibling can expect
//     different spellings of the same argument.
//
// Static analysis, like its sibling: no Tauri runtime, no mocks, and
// therefore nothing that can agree with a bug.
//
// Known blind spot: a call whose command name is a variable rather than a
// literal cannot be resolved statically and is skipped. Same for an argument
// object that is spread or passed by reference. Guessing there would produce
// failures nobody can act on.

import { describe, it, expect } from "vitest";
import ts from "typescript";

const frontendSources = import.meta.glob("../**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

// Discover new command modules automatically; a hand-maintained list silently
// skipped argument validation when a registered command moved to another file.
const rustSources = Object.values(import.meta.glob<string>(["../../src-tauri/src/**/*.rs", "../../setup/src-tauri/src/**/*.rs"], {
  query: "?raw",
  import: "default",
  eager: true,
}));

/** Parameters Tauri injects itself — the frontend never sends them. */
const INJECTED_TYPE = /^(?:tauri::)?(?:AppHandle|State|Window|Webview|WebviewWindow)\b/;

interface CommandSpec {
  /** Argument names as the frontend must spell them. */
  accepted: Set<string>;
  /** Subset of `accepted` that is not `Option<_>` on the Rust side. */
  required: Set<string>;
}

function toCamel(name: string): string {
  const [head, ...rest] = name.split("_");
  return head + rest.map((word) => word.charAt(0).toUpperCase() + word.slice(1)).join("");
}

/** Split a parameter list on commas that are not inside <>, () or []. */
function splitParams(params: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let current = "";
  for (const ch of params) {
    if (ch === "<" || ch === "(" || ch === "[") depth++;
    else if (ch === ">" || ch === ")" || ch === "]") depth--;
    if (ch === "," && depth === 0) {
      out.push(current);
      current = "";
    } else {
      current += ch;
    }
  }
  out.push(current);
  return out;
}

function rustCommands(): Map<string, CommandSpec> {
  const commands = new Map<string, CommandSpec>();
  const re = /#\[(?:tauri::)?command([^\]]*)\](?:\s|\/\/[^\n]*|#\[[^\]]*\])*(?:pub(?:\(crate\))?\s+)?(?:async\s+)?fn\s+(\w+)\s*\(([^)]*)\)/g;
  for (const source of rustSources) {
    for (const [, attr, name, params] of source.matchAll(re)) {
      // Without `rename_all = "snake_case"` Tauri expects camelCase keys.
      const snake = attr.includes("snake_case");
      const accepted = new Set<string>();
      const required = new Set<string>();
      for (const raw of splitParams(params)) {
        const param = raw.trim();
        if (!param) continue;
        const colon = param.indexOf(":");
        const rustName = param.slice(0, colon).trim();
        const rustType = param.slice(colon + 1).trim();
        if (INJECTED_TYPE.test(rustType)) continue;
        const jsName = snake ? rustName : toCamel(rustName);
        accepted.add(jsName);
        if (!param.includes("Option<")) required.add(jsName);
      }
      commands.set(name, { accepted, required });
    }
  }
  return commands;
}

interface Call {
  file: string;
  command: string;
  keys: string[];
}

// Parse call syntax so comments, generic return types and nested values cannot
// hide an argument; calls without an argument object still need validation.
function frontendCalls(sources = frontendSources): Call[] {
  const calls: Call[] = [];
  for (const [file, text] of Object.entries(sources)) {
    if (/\.test\.(ts|tsx)$/.test(file) || file.endsWith(".d.ts")) continue;
    const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true,
      file.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
    function visit(node: ts.Node) {
      if (ts.isCallExpression(node) && ts.isIdentifier(node.expression)
          && ["invoke", "rustInvoke", "tauriInvoke"].includes(node.expression.text)) {
        const [name, args] = node.arguments;
        if (name && ts.isStringLiteralLike(name)
            && (!args || (ts.isObjectLiteralExpression(args)
              && args.properties.every((property) => !ts.isSpreadAssignment(property))))) {
          const keys = args && ts.isObjectLiteralExpression(args)
            ? args.properties.flatMap((property) => {
              const key = property.name;
              return key && (ts.isIdentifier(key) || ts.isStringLiteralLike(key)) ? [key.text] : [];
            }) : [];
          calls.push({ file, command: name.text, keys });
        }
      }
      ts.forEachChild(node, visit);
    }
    visit(source);
  }
  return calls;
}

describe("Tauri command arguments", () => {
  const commands = rustCommands();
  const calls = frontendCalls();

  // Anti-vacuity: a regex that quietly stops matching after a refactor
  // would make every assertion below pass for the wrong reason.
  it("extracts commands and call sites at all", () => {
    expect(commands.size).toBeGreaterThan(20);
    expect(calls.length).toBeGreaterThan(15);
    // A command with a known signature, pinned so a parser regression is
    // loud rather than silent.
    expect(commands.get("save_api_key")?.accepted).toEqual(
      new Set(["key_id", "key", "label"]),
    );
    expect(commands.get("cancel_recording")?.accepted).toEqual(
      new Set(["sessionId"]),
    );
  });

  it("reads signatures with additional attributes and comments", () => {
    expect(commands.get("process_text_ai")?.required).toEqual(new Set(["text"]));
    expect(commands.get("test_ai_prompt")?.accepted.has("profile_id")).toBe(true);
    expect(commands.get("hide")?.required).toEqual(new Set());
    expect(commands.get("hide_tray_popup")?.required).toEqual(new Set());
    expect(commands.get("validate_hotkey")?.required).toEqual(new Set(["hotkey"]));
  });

  it("includes calls without args and preserves keys after comments", () => {
    const parsed = frontendCalls({ "fixture.ts": `
      invoke<string>("process_text_ai");
      invoke("process_text_ai", {
        // A comment cannot hide the required argument.
        text: example({ nested: true }),
        "profile_id": "synthetic"
      });
    ` });
    expect(parsed.map(({ command, keys }) => ({ command, keys }))).toEqual([
      { command: "process_text_ai", keys: [] },
      { command: "process_text_ai", keys: ["text", "profile_id"] },
    ]);
  });

  it("finds a native signature for every inspected frontend call", () => {
    const missing = calls.filter((call) => !commands.has(call.command))
      .map((call) => `${call.file}: ${call.command}`);
    expect(missing, "Argument checks must not silently skip commands").toEqual([]);
  });

  it("passes only arguments the command declares", () => {
    const problems: string[] = [];
    for (const call of calls) {
      const spec = commands.get(call.command);
      if (!spec) continue; // reported by the signature-coverage check above
      const unknown = call.keys.filter((key) => !spec.accepted.has(key));
      if (unknown.length) {
        problems.push(
          `${call.file}: ${call.command} passes ${unknown.join(", ")} — accepts ${[...spec.accepted].join(", ") || "(nothing)"}`,
        );
      }
    }
    expect(problems, problems.join("\n")).toEqual([]);
  });

  it("passes every argument the command requires", () => {
    const problems: string[] = [];
    for (const call of calls) {
      const spec = commands.get(call.command);
      if (!spec) continue;
      const missing = [...spec.required].filter((key) => !call.keys.includes(key));
      if (missing.length) {
        problems.push(`${call.file}: ${call.command} is missing ${missing.join(", ")}`);
      }
    }
    expect(problems, problems.join("\n")).toEqual([]);
  });
});
