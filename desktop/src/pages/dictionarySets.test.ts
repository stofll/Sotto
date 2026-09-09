import { describe, expect, it } from "vitest";
import { dictionaryPatch, parseDictionaryWords, replaceDictionarySet } from "./dictionarySets";
import type { DictionarySet, TextFormattingConfig } from "../bridge/types";

describe("dictionary editing", () => {
  it("accepts pasted lines and commas, preserves phrases and exposes case conflicts", () => {
    expect(parseDictionaryWords(" Claude Code \nRust, Rust\n\nTypeScript,typescript\r\nTauri ")).toEqual(["Claude Code", "Rust", "TypeScript", "typescript", "Tauri"]);
  });

  it("replaces one set without changing other membership or unrelated formatting", () => {
    const a: DictionarySet = { id: "a", name: "Work", description: "", words: ["Rust"], enabled: true };
    const b = { ...a, id: "b", enabled: false };
    const config = { dictionary_sets: [a, b], enabled_presets: ["development"], remove_fillers: false } as TextFormattingConfig;
    const next = replaceDictionarySet(config, { ...a, words: ["Tauri"] });
    expect(next.dictionary_sets).toEqual([{ ...a, words: ["Tauri"] }, b]);
    expect(config.dictionary_sets?.[0].words).toEqual(["Rust"]);
    expect(next.remove_fillers).toBe(false);
    expect(dictionaryPatch(next)).toEqual({ dictionary_sets: next.dictionary_sets, dictionary_spellings: [], enabled_presets: ["development"], custom_words: [] });
  });

  it("keeps UI-only catalog metadata out of persisted copies", () => {
    const copy = { id: "copy", name: "Copy", description: "", words: ["Rust"], enabled: false, builtin: false };
    const patch = dictionaryPatch({ dictionary_sets: [copy], enabled_presets: [] } as unknown as TextFormattingConfig);
    expect(patch.dictionary_sets).toEqual([{ id: "copy", name: "Copy", description: "", words: ["Rust"], enabled: false }]);
    expect(copy).toHaveProperty("builtin", false);
  });
});
