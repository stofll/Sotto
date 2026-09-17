import { useTranslations, defaultLocale } from './index';
import type { Dictionary, Locale } from './types';

/**
 * Resolves the dictionary for the page being rendered. Components call this
 * instead of receiving `t` through props, which keeps deeply nested sections
 * from having to thread it down by hand.
 */
export const translationsFor = (currentLocale: string | undefined): { locale: Locale; t: Dictionary } => {
  const locale = (currentLocale === 'ru' ? 'ru' : defaultLocale) as Locale;
  return { locale, t: useTranslations(locale) };
};
