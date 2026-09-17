// @ts-check
import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';

// The canonical origin. Astro needs it for sitemap and canonical URLs, so it is
// the one place a domain change has to be made.
export const SITE = 'https://sotto.app';

export default defineConfig({
  site: SITE,
  // English stays at the root so the existing URL keeps working; Russian lives
  // under /ru/. `prefixDefaultLocale: false` is what keeps `/` free of `/en/`.
  i18n: {
    locales: ['en', 'ru'],
    defaultLocale: 'en',
    routing: { prefixDefaultLocale: false },
  },
  integrations: [sitemap({ i18n: { defaultLocale: 'en', locales: { en: 'en', ru: 'ru' } } })],
  build: { inlineStylesheets: 'auto' },
});
