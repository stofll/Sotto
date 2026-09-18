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
//   • the casing rule is not decorative — Tauri maps a Rust `old_hotkey`
//     to `oldHotkey` in JS unless the command opts into
//     `rename_all = "snake_case"`, so a command and its sibling can expect
//     different spellings of the same argument.
//
// Static analysis, like its sibling: no Tauri runtime, no mocks, and
// therefore nothing that can agree with a bug.
//
// Known blind spot: a call whose command name is a variable rather than a
// literal (tauriInvoke(command, …) in HotkeyDisplay, which serves both
// hotkey commands from one component) cannot be resolved statically and is
// skipped. Same for an argument object that is spread or passed by
// reference. Guessing there would produce failures nobody can act on.

import { describe, it, expect } from "vitest";

const frontendSources = import.meta.glob("../**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

import libRs from "../../src-tauri/src/lib.rs?raw";
import formatCommandsRs from "../../src-tauri/src/format_commands.rs?raw";
import audioWorkerRs from "../../src-tauri/src/audio_worker.rs?raw";

import overlayRs from "../../src-tauri/src/overlay.rs?raw";
import hotkeyRs from "../../src-tauri/src/hotkey.rs?raw";
import updaterRs from "../../src-tauri/src/updater.rs?raw";
import soundsRs from "../../src-tauri/src/sounds.rs?raw";
import outputVolumeRs from "../../src-tauri/src/output_volume.rs?raw";
import micTestRs from "../../src-tauri/src/mic_test.rs?raw";
import debugRs from "../../src-tauri/src/debug.rs?raw";
import dictionariesRs from "../../src-tauri/src/dictionaries.rs?raw";
import accessibilityRs from "../../src-tauri/src/accessibility.rs?raw";
import clipboardRs from "../../src-tauri/src/clipboard.rs?raw";
import statsRs from "../../src-tauri/src/stats.rs?raw";
import configRs from "../../src-tauri/src/config.rs?raw";
import audioRs from "../../src-tauri/src/audio.rs?raw";
import secretStoreRs from "../../src-tauri/src/secret_store.rs?raw";
import historyRs from "../../src-tauri/src/history.rs?raw";
import modelRs from "../../src-tauri/src/model.rs?raw";
import modelDownloadRs from "../../src-tauri/src/model_download.rs?raw";
import dictationRs from "../../src-tauri/src/dictation.rs?raw";
import audioFileRs from "../../src-tauri/src/audio_file.rs?raw";
import aiRs from "../../src-tauri/src/ai/mod.rs?raw";
const rustSources = [
  libRs,
  formatCommandsRs,
  audioWorkerRs,
  overlayRs,
  hotkeyRs,
  updaterRs,
  soundsRs,
  outputVolumeRs,
  micTestRs,
  debugRs,
  dictionariesRs,
  accessibilityRs,
  clipboardRs,
  statsRs,
  configRs,
  audioRs,
  secretStoreRs,
  historyRs,
  modelRs,
  modelDownloadRs,
  dictationRs,
  audioFileRs,
  aiRs,
];

/** Parameters Tauri injects itself — the frontend never sends them. */
const INJECTED = new Set(["app", "state", "window", "webview", "app_handle"]);

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
  const re = /#\[tauri::command([^\]]*)\]\s*(?:pub(?:\(crate\))?\s+)?(?:async\s+)?fn\s+(\w+)\s*\(([^)]*)\)/g;
  for (const source of rustSources) {
    for (const [, attr, name, params] of source.matchAll(re)) {
      // Without `rename_all = "snake_case"` Tauri expects camelCase keys.
      const snake = attr.includes("snake_case");
      const accepted = new Set<string>();
      const required = new Set<string>();
      for (const raw of splitParams(params)) {
        const param = raw.trim();
        if (!param) continue;
        const rustName = param.split(":")[0].trim();
        if (INJECTED.has(rustName)) continue;
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

/** Keys of an object literal, top level only — nested objects are values. */
function topLevelKeys(body: string): string[] {
  const keys: string[] = [];
  let depth = 0;
  let atKeyPosition = true;
  let token = "";
  for (let i = 0; i < body.length; i++) {
    const ch = body[i];
    if (ch === "{" || ch === "[" || ch === "(") depth++;
    else if (ch === "}" || ch === "]" || ch === ")") depth--;
    if (depth !== 0) continue;
    if (ch === ",") {
      // Shorthand property (`{ sessionId }`) — the token is the key.
      if (atKeyPosition && token.trim()) keys.push(token.trim());
      token = "";
      atKeyPosition = true;
      continue;
    }
    if (ch === ":" && atKeyPosition) {
      keys.push(token.trim());
      token = "";
      atKeyPosition = false;
      continue;
    }
    token += ch;
  }
  if (atKeyPosition && token.trim()) keys.push(token.trim());
  return keys.filter((key) => /^[A-Za-z_]\w*$/.test(key));
}

function frontendCalls(): Call[] {
  const calls: Call[] = [];
  // Only calls whose second argument is an inline object literal: a spread
  // or a variable cannot be checked statically, and guessing would produce
  // failures nobody can act on.
  const re = /\b(?:invoke|rustInvoke|tauriInvoke)\b[^("'`\n]*\(\s*["'`](\w+)["'`]\s*,\s*\{/g;
  for (const [path, text] of Object.entries(frontendSources)) {
    if (/\.test\.(ts|tsx)$/.test(path) || path.endsWith(".d.ts")) continue;
    for (const match of text.matchAll(re)) {
      const open = match.index! + match[0].length - 1;
      let depth = 0;
      let close = -1;
      for (let i = open; i < text.length; i++) {
        if (text[i] === "{") depth++;
        else if (text[i] === "}") {
          depth--;
          if (depth === 0) {
            close = i;
            break;
          }
        }
      }
      if (close === -1) continue;
      calls.push({
        file: path,
        command: match[1],
        keys: topLevelKeys(text.slice(open + 1, close)),
      });
    }
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
    expect(commands.get("set_hotkey")?.accepted).toEqual(
      new Set(["hotkey", "oldHotkey"]),
    );
  });

  it("passes only arguments the command declares", () => {
    const problems: string[] = [];
    for (const call of calls) {
      const spec = commands.get(call.command);
      if (!spec) continue; // covered by command-surface.test.ts
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
