import { beforeEach, describe, expect, it, vi } from "vitest";
import { analyzeDictionary, getDictionaryPresets, getParasiteSets } from "./dictionaries";
import type { TextFormattingConfig } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("./invoke", () => ({ invoke }));
beforeEach(() => { invoke.mockReset(); });

describe("dictionary bridge", () => {
  it("returns the complete built-in contents without changing configuration", async () => {
    invoke.mockResolvedValue([["development", ["Rust", "Claude Code"]]]);
    expect(await getDictionaryPresets()).toEqual([["development", ["Rust", "Claude Code"]]]);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("dictionary_presets");
  });
  it("hands over the built-in parasite sets so settings can show them", async () => {
    const sets = [{ id: "ru", language: "ru", words: ["ну", "типа"], default_on: true }];
    invoke.mockResolvedValue(sets);
    expect(await getParasiteSets()).toEqual(sets);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("parasite_sets");
  });
  it("checks the unsaved candidate and preserves conflict and support metadata", async () => {
    const formatting = { dictionary_sets: [{ id: "a", name: "Work", description: "", enabled: true, words: ["Node", "Rust", "rust"] }], dictionary_spellings: [] } as unknown as TextFormattingConfig;
    const result = { effective_count: 2, conflicts: [{ key: "rust", variants: ["Rust", "rust"], selected: null }], unsupported_words: ["Node"] };
    invoke.mockResolvedValue(result);
    expect(await analyzeDictionary(formatting)).toEqual(result);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("analyze_dictionary", { formatting });
  });
  it("propagates load and analysis failures so the UI can retry", async () => {
    invoke.mockRejectedValue(new Error("offline"));
    await expect(getDictionaryPresets()).rejects.toThrow("offline");
    await expect(analyzeDictionary({} as TextFormattingConfig)).rejects.toThrow("offline");
  });
});
