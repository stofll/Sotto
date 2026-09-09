import { invoke } from "./invoke";
import type { DictionaryAnalysis, TextFormattingConfig } from "./types";

export function getDictionaryPresets(): Promise<[string, string[]][]> {
  return invoke("dictionary_presets");
}

export function analyzeDictionary(formatting: TextFormattingConfig): Promise<DictionaryAnalysis> {
  return invoke("analyze_dictionary", { formatting });
}
