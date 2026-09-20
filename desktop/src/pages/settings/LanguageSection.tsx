import { useEffect, useMemo, useState } from "react";
import { Segmented } from "../../components/Shell";
import { CustomSelect, type SelectOption } from "../../components/CustomSelect";
import { applyLocaleFromConfig, getLocale, isLocale, LOCALE_LABELS, LOCALES, t, type Locale } from "../../i18n";
import type { ModelInfo } from "../../bridge/types";
import { fallbackLanguage, speechLanguages } from "../modelCatalog";
import type { ConfigChanged } from "./controls";

// The UI language. Deliberately separate from LanguagePicker: that one is about
// the speech language, and confusing the two is expensive — picking «English» in
// the hope of switching the interface breaks Russian dictation.
export function UiLanguagePicker({ value, onConfigChanged }: { value?: Locale; onConfigChanged: ConfigChanged }) {
  const current = value ?? getLocale();
  const [saving, setSaving] = useState(false);
  // The wrapper exists for the height: Segmented is styled inline, and its
  // buttons can only be reached through a class from outside.
  return (
    <div className="lang-row__ui-language">
      <Segmented
        value={current}
        disabled={saving}
        options={LOCALES.map((locale) => ({ value: locale, label: LOCALE_LABELS[locale] }))}
        onChange={(next) => {
          if (!isLocale(next) || saving || next === current) return;
          // The saved config updates every window's locale. A failed write
          // must leave this window in the same language as the others.
          setSaving(true);
          void onConfigChanged({ ui_language: next }).then((saved) => {
            if (saved) applyLocaleFromConfig(saved.ui_language);
          }).finally(() => setSaving(false));
        }}
      />
    </div>
  );
}

export function LanguagePicker({ language, model, models, onConfigChanged }: { language?: string; model?: ModelInfo; models: ModelInfo[]; onConfigChanged: ConfigChanged }) {
  // A model with a closed language list decides for that list itself: both
  // English-only Whisper builds and GigaAM produce garbage rather than an error
  // on a foreign language.
  const correction = fallbackLanguage(model, language);
  useEffect(() => {
    if (correction) void onConfigChanged({ language: correction });
  }, [correction, onConfigChanged]);
  // The same list as in the catalog: it would be odd to be able to download a
  // German model yet unable to say you are dictating in German. There are
  // deliberately no flags — a language is not a country, and English and Arabic
  // have a dozen each.
  const options = useMemo<Array<SelectOption<string>>>(
    () => [
      { value: "auto", label: t("Авто"), icon: "globe" },
      ...speechLanguages(model, models).map((item) => ({
        value: item.code,
        label: item.name,
        meta: item.code.toUpperCase(),
      })),
    ],
    [model, models],
  );
  const value = correction ?? language ?? "ru";
  return <CustomSelect className="custom-select--language" value={value} options={options} searchable inlineMeta onChange={(next) => void onConfigChanged({ language: next })}/>;
}
