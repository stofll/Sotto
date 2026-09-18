import type { APIRoute } from 'astro';

/**
 * Generated rather than kept in public/, so the sitemap URL follows `site` in
 * astro.config.mjs and cannot be left pointing at a stale domain.
 */
export const GET: APIRoute = ({ site }) =>
  new Response(
    `User-agent: *\nAllow: /\n\nSitemap: ${new URL('sitemap-index.xml', site)}\n`,
    { headers: { 'Content-Type': 'text/plain; charset=utf-8' } },
  );
