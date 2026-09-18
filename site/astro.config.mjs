// @ts-check
import { rename, rm } from 'node:fs/promises';
import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';

// The canonical origin. Astro needs it for sitemap and canonical URLs, so it is
// the one place a domain change has to be made.
export const SITE = 'https://sotto.app';

/** Routing, the sitemap and the 404 plugin below are all derived from this.
 *  The same list is repeated in src/i18n/index.ts, because the client bundle
 *  cannot import this config, so the two are edited together. */
const LOCALES = ['en', 'ru'];
const DEFAULT_LOCALE = 'en';
/** Locales carrying a path prefix. The default locale has none; see i18n below. */
const PREFIXED_LOCALES = LOCALES.filter((locale) => locale !== DEFAULT_LOCALE);

/**
 * Astro emits the root `404.astro` as `dist/404.html` but treats a nested one
 * as an ordinary route, so `src/pages/ru/404.astro` lands in `dist/ru/404/`.
 * Cloudflare answers a miss with the nearest file literally named `404.html`,
 * walking up the tree, so the nested directory is invisible to it and every
 * Russian miss would fall back to the English page. Flattening it is what makes
 * `/ru/<anything-wrong>` answer in Russian.
 */
function flattenLocaleNotFound() {
  return {
    name: 'sotto:flatten-locale-404',
    hooks: {
      'astro:build:done': async ({ dir, logger }) => {
        for (const locale of PREFIXED_LOCALES) {
          const nested = new URL(`${locale}/404/index.html`, dir);
          const flat = new URL(`${locale}/404.html`, dir);
          try {
            await rename(nested, flat);
          } catch (cause) {
            // Failing the build is deliberate: without this file, misses under
            // /<locale>/ would quietly answer with the English page, and the
            // only place to notice that would be production.
            throw new Error(
              `Locale ${locale} is declared, but src/pages/${locale}/404.astro did not build. ` +
                `Add the page, or drop the locale from LOCALES in astro.config.mjs.`,
              { cause },
            );
          }
          await rm(new URL(`${locale}/404/`, dir), { recursive: true, force: true });
          logger.info(`${locale}/404/index.html -> ${locale}/404.html`);
        }
      },
    },
  };
}

export default defineConfig({
  site: SITE,
  // English stays at the root so the existing URL keeps working; Russian lives
  // under /ru/. `prefixDefaultLocale: false` is what keeps `/` free of `/en/`.
  i18n: {
    locales: LOCALES,
    defaultLocale: DEFAULT_LOCALE,
    routing: { prefixDefaultLocale: false },
  },
  integrations: [
    flattenLocaleNotFound(),
    sitemap({
      i18n: {
        defaultLocale: DEFAULT_LOCALE,
        locales: Object.fromEntries(LOCALES.map((locale) => [locale, locale])),
      },
      // The integration emits one xhtml:link per locale but has no notion of
      // x-default, so the sitemap would disagree with the <head>, which does
      // declare it. Point it at the English URL, exactly as the pages do.
      serialize: (item) => {
        const english = item.links?.find((link) => link.lang === DEFAULT_LOCALE);
        if (english) item.links = [...item.links, { lang: 'x-default', url: english.url }];
        return item;
      },
    }),
  ],
  build: { inlineStylesheets: 'auto' },
});
