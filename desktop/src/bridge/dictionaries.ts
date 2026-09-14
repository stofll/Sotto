import { invoke } from "./invoke";
import type { DictionaryAnalysis, TextFormattingConfig } from "./types";

export function getDictionaryPresets(): Promise<[string, string[]][]> {
  return invoke("dictionary_presets");
}

/** The built-in parasite words the cleanup removes, in the order the step applies them. */
export function getParasiteWords(): Promise<string[]> {
  return invoke("parasite_words");
}

export function analyzeDictionary(formatting: TextFormattingConfig): Promise<DictionaryAnalysis> {
  return invoke("analyze_dictionary", { formatting });
}
