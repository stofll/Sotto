import type { APIRoute } from 'astro';
import { appLogoRows } from '../data/app-logos';

/**
 * The app marks as one cached sprite. The band repeats each symbol for its
 * loop, so inlining the paths would put every one of them in the HTML.
 */
export const GET: APIRoute = () => {
  const symbols = appLogoRows.flat().map(({ id, path }) => `<symbol id="${id}" viewBox="0 0 24 24"><path d="${path}"/></symbol>`);
  return new Response(`<svg xmlns="http://www.w3.org/2000/svg">${symbols.join('')}</svg>`, {
    headers: { 'Content-Type': 'image/svg+xml' },
  });
};
