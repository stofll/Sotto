import { invoke } from "./invoke";
import type { DictionaryAnalysis, TextFormattingConfig } from "./types";

export function getDictionaryPresets(): Promise<[string, string[]][]> {
  return invoke("dictionary_presets");
}

/** A built-in parasite word set: one language's list, as the backend ships it. */
export type ParasiteSet = {
  id: string;
  /** The dictation language the set is written for. */
  language: string;
  words: string[];
  /** Whether the set applies to a config nobody has configured. */
  default_on: boolean;
};

/** The built-in parasite word sets, in the order settings should list them. */
export function getParasiteSets(): Promise<ParasiteSet[]> {
  return invoke("parasite_sets");
}

export function analyzeDictionary(formatting: TextFormattingConfig): Promise<DictionaryAnalysis> {
  return invoke("analyze_dictionary", { formatting });
}
