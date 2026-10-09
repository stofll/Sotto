// @ts-check
import { readFile, rename, rm } from 'node:fs/promises';
import { relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';

// The canonical origin. Astro needs it for sitemap and canonical URLs, so it is
// the one place a domain change has to be made.
export const SITE = 'https://sotto.today';

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

const DESKTOP_SRC = fileURLToPath(new URL('../desktop/src/', import.meta.url));
const SITE_ROOT = fileURLToPath(new URL('./', import.meta.url));
const posix = (/** @type {string} */ path) => path.replaceAll('\\', '/');

/** React as the app's components import it, and its Preact equivalent. */
const PREACT = {
  react: 'preact/compat',
  'react-dom': 'preact/compat',
  'react-dom/client': 'preact/compat/client',
  'react/jsx-runtime': 'preact/jsx-runtime',
  'react/jsx-dev-runtime': 'preact/jsx-dev-runtime',
};
/** The prefix of the app's stylesheets, which load with the overlay's chunk. */
const LAZY_CSS = '\0sotto-app-overlay-css:';
/** App modules the page cannot run (Tauri events, the app's dictionaries), by path under desktop/src/. */
const STUBS = {
  'bridge/events.ts': 'src/scripts/app-overlay/bridge-events.ts',
  'i18n/index.ts': 'src/scripts/app-overlay/i18n.ts',
};

/**
 * The hero draws the app's own overlay from desktop/src/overlay rather than a
 * copy, so the two cannot drift. Imports made from the app's sources resolve
 * against the site's install: React becomes Preact, and the modules above
 * become the site's stand-ins. The site therefore needs neither the app's
 * node_modules nor its English dictionary. The overlay's stylesheets arrive
 * with its chunk and are added when it loads, rather than inlined into every
 * page's first paint.
 */
function appOverlay() {
  return {
    name: 'sotto:app-overlay',
    enforce: /** @type {const} */ ('pre'),
    /** @param {string} source @param {string | undefined} importer @param {any} options @this {any} */
    async resolveId(source, importer, options) {
      if (!importer || !posix(importer).startsWith(posix(DESKTOP_SRC))) return null;
      if (source in PREACT) {
        return this.resolve(PREACT[/** @type {keyof typeof PREACT} */ (source)], `${SITE_ROOT}package.json`, { ...options, skipSelf: true });
      }
      const resolved = await this.resolve(source, importer, { ...options, skipSelf: true });
      if (resolved && resolved.id.endsWith('.css')) return `${LAZY_CSS}${resolved.id}.js`;
      const stub = resolved && STUBS[/** @type {keyof typeof STUBS} */ (posix(relative(DESKTOP_SRC, resolved.id.split('?')[0])))];
      return stub ? fileURLToPath(new URL(stub, import.meta.url)) : resolved;
    },
    /** @param {string} id */
    async load(id) {
      if (!id.startsWith(LAZY_CSS)) return null;
      const css = (await readFile(id.slice(LAZY_CSS.length, -3), 'utf8')).replace(/\/\*[\s\S]*?\*\//g, '').replace(/\s+/g, ' ');
      return `document.head.append(Object.assign(document.createElement('style'), { textContent: ${JSON.stringify(css)} }));`;
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
  // The whole stylesheet is small enough to ship inside the page, which saves
  // the render-blocking request Lighthouse measured at up to a second on a
  // slow mobile connection.
  build: { inlineStylesheets: 'always' },
  vite: {
    plugins: [appOverlay()],
    // The dev server serves the app's overlay sources from outside the site.
    server: { fs: { allow: [SITE_ROOT, DESKTOP_SRC] } },
    // Found only once the overlay loads; listed so the dev server does not
    // re-bundle them mid-session and fail that first load.
    optimizeDeps: { include: ['preact', 'preact/hooks', 'preact/compat', 'preact/compat/client', 'preact/jsx-runtime', 'preact/jsx-dev-runtime'] },
  },
});
