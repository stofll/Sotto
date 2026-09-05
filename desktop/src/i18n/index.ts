//! UI localization.
//!
//! The key is the Russian text itself, as in gettext. There are deliberately
//! no separate identifiers: a key registry has to be maintained in parallel
//! with the code, it drifts apart, and `t("settings.autoPaste.hint")` cannot be
//! proof-read by eye. Here a missing translation falls back to the key, that is
//! to the Russian original — the worst case is mixed language, not a blank
//! button.
//!
//! The price: editing the Russian copy breaks the link to the translation.
//! That is caught by `check-i18n.mjs`, which reconciles the keys in the code
//! with the keys in the dictionary.
//!
//! What does NOT belong here: LLM system prompts, the dictation samples in the
//! formatting preview, and the filler-word dictionary. Those belong to the
//! language of speech, not of the interface, and must not switch along with it.

import { useSyncExternalStore } from "react";

import { en } from "./en";

export const LOCALES = ["ru", "en"] as const;
export type Locale = (typeof LOCALES)[number];

export const LOCALE_LABELS: Record<Locale, string> = {
  ru: "Русский",
  en: "English",
};

/** Dictionary: key (Russian) → translation. An array holds plural forms. */
export type Dictionary = Record<string, string | string[]>;

const DICTIONARIES: Record<Locale, Dictionary | null> = {
  ru: null, // Russian is the keys themselves, no dictionary needed
  en,
};

let current: Locale = "ru";
const listeners = new Set<() => void>();

export function getLocale(): Locale {
  return current;
}

export function setLocale(locale: Locale) {
  if (locale === current) return;
  current = locale;
  document.documentElement.lang = locale;
  for (const listener of listeners) listener();
}

export function subscribeLocale(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * Subscription to a language change. Calling it once at the root is enough:
 * `t()` reads module state, so re-rendering the root is sufficient for the whole
 * tree to update. A separate hook in every component would only multiply
 * subscriptions to no benefit.
 */
export function useLocale(): Locale {
  return useSyncExternalStore(subscribeLocale, getLocale, getLocale);
}

/** The system language, if we know it. Otherwise Russian — the original. */
export function detectLocale(): Locale {
  const tags = typeof navigator === "undefined" ? [] : navigator.languages ?? [navigator.language];
  for (const tag of tags) {
    const base = tag.toLowerCase().split("-")[0];
    if ((LOCALES as readonly string[]).includes(base)) return base as Locale;
  }
  return "ru";
}

export function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && (LOCALES as readonly string[]).includes(value);
}

/**
 * Apply the language from config. An empty value means "follow the system":
 * old configs have no such field at all, and forcing Russian on them merely
 * because the app was written in Russian would be wrong.
 */
export function applyLocaleFromConfig(value: unknown) {
  setLocale(isLocale(value) ? value : detectLocale());
}

/**
 * Tag for `toLocaleDateString`. Dates and numbers must follow the UI language,
 * otherwise you get "Aug 15" in the Russian build.
 */
export function localeTag(): string {
  return current === "ru" ? "ru-RU" : "en-US";
}

/**
 * Substitutes `{name}` from `params`. A placeholder with no value is left as
 * is: a visible `{count}` in the UI is a bug that should be noticed, while a
 * silent blank is a bug that will not be.
 */
function interpolate(template: string, params?: Record<string, string | number>): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in params ? String(params[name]) : whole,
  );
}

function lookup(key: string): string | string[] | undefined {
  return DICTIONARIES[current]?.[key];
}

/** Translate a string. The key is the Russian original. */
export function t(key: string, params?: Record<string, string | number>): string {
  const found = lookup(key);
  const template = typeof found === "string" ? found : key;
  return interpolate(template, params);
}

/**
 * Plural forms.
 *
 * Russian needs three forms, English two, and picking them requires knowing the
 * language — which is why this is a separate function rather than `t()` with a
 * parameter. `forms` holds the Russian forms (one / two / five); a translation
 * keeps its own in an array of whatever length its language needs.
 */
export function tPlural(count: number, forms: [string, string, string], params?: Record<string, string | number>): string {
  const found = lookup(forms.join("|"));
  const localized = Array.isArray(found) ? found : forms;
  const index = pluralIndex(count, current, localized.length);
  return interpolate(localized[index] ?? localized[localized.length - 1], { count, ...params });
}

/** The key under which `tPlural` looks up forms. Needed by the check script. */
export function pluralKey(forms: [string, string, string]): string {
  return forms.join("|");
}

function pluralIndex(count: number, locale: Locale, formCount: number): number {
  if (locale === "ru" || formCount === 3) {
    const mod10 = count % 10;
    const mod100 = count % 100;
    if (mod10 === 1 && mod100 !== 11) return 0;
    if (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return 1;
    return 2;
  }
  return count === 1 ? 0 : 1;
}
