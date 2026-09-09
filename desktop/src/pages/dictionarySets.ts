import type { DictionarySet, TextFormattingConfig } from "../bridge/types";
import { t } from "../i18n";

export function dictionaryName(set: Pick<DictionarySet, "id" | "name">): string {
  return set.name || (set.id === "legacy-personal" ? t("Мои слова") : set.id);
}

export function parseDictionaryWords(text: string): string[] {
  return [...new Set(text.split(/[\n,]/).map((word) => word.trim()).filter(Boolean))];
}

export function dictionaryPatch(formatting: TextFormattingConfig): Partial<TextFormattingConfig> {
  return {
    dictionary_sets: (formatting.dictionary_sets ?? []).map(({ id, name, description, enabled, words }) => ({ id, name, description, enabled, words })),
    dictionary_spellings: formatting.dictionary_spellings ?? [],
    enabled_presets: formatting.enabled_presets,
    custom_words: [],
  };
}

export function replaceDictionarySet(formatting: TextFormattingConfig, set: DictionarySet): TextFormattingConfig {
  const sets = formatting.dictionary_sets ?? [];
  return { ...formatting, dictionary_sets: sets.some((item) => item.id === set.id)
    ? sets.map((item) => item.id === set.id ? set : item) : [...sets, set] };
}
