import { invoke } from "./invoke";
import type { DictionaryAnalysis, DictionaryMatch, TermMatching, TextFormattingConfig } from "./types";

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

/** What each term reacts to; `builtin` is whether the terms come from a built-in set. */
export function getTermMatching(words: string[], builtin: boolean): Promise<TermMatching[]> {
  return invoke("term_matching", { words, builtin });
}

/** What one set alone makes of `text`, run as a built-in set when `builtin`. */
export function checkDictionaryText(text: string, words: string[], builtin: boolean): Promise<DictionaryMatch[]> {
  return invoke("check_dictionary_text", { text, words, builtin });
}

export function analyzeDictionary(formatting: TextFormattingConfig): Promise<DictionaryAnalysis> {
  return invoke("analyze_dictionary", { formatting });
}
