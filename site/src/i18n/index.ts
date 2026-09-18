import { en } from './en';
import { ru } from './ru';
import { privacyEn, privacyRu, type PrivacyPage } from './privacy';
import type { Dictionary, Locale } from './types';

/** Must match LOCALES/DEFAULT_LOCALE in astro.config.mjs. Routing is declared
 *  there, but these values are needed in the client bundle, which cannot import
 *  the config. Adding a locale means editing both, plus src/pages/<locale>/. */
export const locales: Locale[] = ['en', 'ru'];
export const defaultLocale: Locale = 'en';

const dictionaries: Record<Locale, Dictionary> = { en, ru };

export const useTranslations = (locale: Locale): Dictionary => dictionaries[locale];

/** Root-relative path for a locale, optionally for a sub-page. English is served from `/`. */
export const localePath = (locale: Locale, slug = ''): string => {
  const prefix = locale === defaultLocale ? '/' : `/${locale}/`;
  return slug ? `${prefix}${slug}/` : prefix;
};

export const privacyCopy = (locale: Locale): PrivacyPage => (locale === 'ru' ? privacyRu : privacyEn);

/** Replaces `{name}` placeholders. Keeps formatting out of the markup. */
export const format = (template: string, values: Record<string, string | number>): string =>
  template.replace(/\{(\w+)\}/g, (match, key: string) => String(values[key] ?? match));

export type { Dictionary, Locale, PrivacyPage };
