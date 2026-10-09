/**
 * Stands in for desktop/src/i18n/index.ts when the site builds the app's
 * overlay. The site passes the overlay its own strings; the few the overlay
 * looks up itself fall back to their Russian keys, as the app's do, and the
 * app's English dictionary is never bundled. See astro.config.mjs.
 */
export function t(key: string, params?: Record<string, string | number>): string {
  return params ? key.replace(/\{(\w+)\}/g, (match, name: string) => String(params[name] ?? match)) : key;
}
