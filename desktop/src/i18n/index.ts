//! Локализация интерфейса.
//!
//! Ключ — сам русский текст, как в gettext. Своих идентификаторов нет
//! намеренно: реестр ключей надо вести параллельно с кодом, он расходится,
//! и `t("settings.autoPaste.hint")` невозможно вычитать глазами. Здесь
//! отсутствующий перевод падает на ключ, то есть на русский оригинал —
//! худший случай это смешанный язык, а не пустая кнопка.
//!
//! Цена: правка русской копии рвёт связь с переводом. Это ловится скриптом
//! `check-i18n.mjs`, который сверяет ключи в коде с ключами в словаре.
//!
//! Что сюда НЕ попадает: системные промпты LLM, примеры диктовки в
//! предпросмотре форматирования и словарь слов-паразитов. Они относятся к
//! языку речи, а не интерфейса, и переключаться вместе с ним не должны.

import { useSyncExternalStore } from "react";

import { en } from "./en";

export const LOCALES = ["ru", "en"] as const;
export type Locale = (typeof LOCALES)[number];

export const LOCALE_LABELS: Record<Locale, string> = {
  ru: "Русский",
  en: "English",
};

/** Словарь: ключ (русский) → перевод. Массив — формы множественного числа. */
export type Dictionary = Record<string, string | string[]>;

const DICTIONARIES: Record<Locale, Dictionary | null> = {
  ru: null, // русский — это сами ключи, словарь не нужен
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
 * Подписка на смену языка. Достаточно вызвать один раз в корне: `t()`
 * читает модульное состояние, так что перерисовки корня хватает, чтобы
 * обновилось всё дерево. Отдельный хук в каждом компоненте только
 * размножил бы подписки без пользы.
 */
export function useLocale(): Locale {
  return useSyncExternalStore(subscribeLocale, getLocale, getLocale);
}

/** Язык системы, если он нам известен. Иначе русский — язык оригинала. */
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
 * Применить язык из конфига. Пустое значение — «как в системе»: у старых
 * конфигов поля нет вовсе, и подставлять им русский только потому, что
 * приложение писалось на русском, неправильно.
 */
export function applyLocaleFromConfig(value: unknown) {
  setLocale(isLocale(value) ? value : detectLocale());
}

/**
 * Тег для `toLocaleDateString`. Даты и числа должны следовать за языком
 * интерфейса, иначе получается «Aug 15» в русской версии.
 */
export function localeTag(): string {
  return current === "ru" ? "ru-RU" : "en-US";
}

/**
 * Подставляет `{name}` из `params`. Плейсхолдер без значения остаётся как
 * есть: видимая `{count}` в интерфейсе — это баг, который надо заметить, а
 * молчаливая пустота — баг, который не заметят.
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

/** Перевести строку. Ключ — русский оригинал. */
export function t(key: string, params?: Record<string, string | number>): string {
  const found = lookup(key);
  const template = typeof found === "string" ? found : key;
  return interpolate(template, params);
}

/**
 * Формы множественного числа.
 *
 * Русский требует три формы, английский две, и подобрать их можно только
 * зная язык — поэтому это отдельная функция, а не `t()` с параметром.
 * `forms` — русские формы (одна / две / пять); перевод хранит свои в
 * массиве той длины, которая нужна его языку.
 */
export function tPlural(count: number, forms: [string, string, string], params?: Record<string, string | number>): string {
  const found = lookup(forms.join("|"));
  const localized = Array.isArray(found) ? found : forms;
  const index = pluralIndex(count, current, localized.length);
  return interpolate(localized[index] ?? localized[localized.length - 1], { count, ...params });
}

/** Ключ, под которым `tPlural` ищет формы. Нужен скрипту проверки. */
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
