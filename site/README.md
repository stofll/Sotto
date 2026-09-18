# Sotto website

The landing page for [Sotto](https://github.com/stofll/Sotto), built with Astro and served as static HTML.

It lives in this repository but installs separately: `site/` has its own `pnpm-lock.yaml` and is **not** a member of the `desktop/` workspace. Changing the landing page cannot touch the app's dependency graph, and `rust-ci.yml` and `ui-tests.yml` ignore `site/**` so a copy edit does not run the three-OS build matrix.

## Commands

```bash
pnpm install     # from this directory
pnpm dev         # http://localhost:4321
pnpm build       # static output in dist/
pnpm typecheck   # astro check
pnpm brand       # re-copy the icon from the desktop app
pnpm glyph       # re-trace the flat web mark from it
pnpm og          # regenerate the social preview image
```

## Layout

`src/data/product.ts` holds facts that are not translated: links, model names, download sizes. The model figures come from [`docs/models.md`](../docs/models.md) — change them there first.

`src/i18n/` holds everything a translator touches. `types.ts` is the contract: adding a string there makes `pnpm typecheck` fail for any locale that has not translated it yet, which is what keeps English and Russian from drifting apart. Russian terminology follows [`README.ru.md`](../README.ru.md) — «диктовка», «распознавание», «оверлей», «горячая клавиша».

## Brand and colour

`desktop/src-tauri/icons/icon-source-1024.png` is the only source of truth for the mark, and the three brand commands derive everything else from it. The site had drifted onto the previous icon precisely because its copy was hand-placed; do not hand-place another one.

The page uses the bare ribbon, not the app icon: `pnpm glyph` traces it into `src/assets/sotto-glyph.svg` as two flat tones of `currentColor`, so it scales, recolours, and costs a fraction of the render. The plate stays where a plate belongs — the favicon and the social image.

Colour follows the mark. Every value sits on the mark's own axis (hue -100, chroma capped at 0.018), which is why the page is almost neutral. One hue survives, `--accent`, and it is reserved for live state: the recording dots, the typing carets, the "Local" indicator, the waveform, and the pulse along the on-device path. If you find yourself reaching for it anywhere else, reach for `--silver` instead.

## Type

Sizes are in rem and the root size is never pinned, so a reader who has enlarged their browser default gets a larger page. Page copy has a floor of 12px and letter-spaced labels a floor of 11px; the only text allowed below that is inside the mocked application windows, which are meant to read as a scaled-down interface.

`src/components/` renders the markup. Sections read their strings through `translationsFor(Astro.currentLocale)` rather than receiving them as props.

`src/scripts/` is progressive enhancement, one module per section. Nothing on the page requires it: with JavaScript disabled the copy, the links and the downloads all still work, and the demos simply hold still. The client scripts stay locale-agnostic by reading the strings that `RuntimeStrings.astro` renders into the page as JSON.

`src/styles/` is the stylesheet split by concern, imported in order by `global.css`. Load order matters: tokens first, breakpoints last.

## Routing

English is served from `/`, Russian from `/ru/`. Adding a locale means adding it to `astro.config.mjs`, writing `src/i18n/<locale>.ts`, and creating `src/pages/<locale>/index.astro`.

Each locale also has a 404 page, and getting it to the right place takes one build step. Cloudflare answers a miss with the nearest file literally named `404.html`, walking up the tree — but Astro only special-cases the root `404.astro`, and emits a nested one as `dist/ru/404/index.html`, which that lookup cannot see. The `sotto:flatten-locale-404` integration in `astro.config.mjs` renames it to `dist/ru/404.html`, which is what makes `/ru/<anything-wrong>` answer in Russian instead of falling back to English. A new locale has to be listed there too.

Both 404 pages pass `noindex` to `Base.astro`, which drops the canonical link, the hreflang set and the structured data. All three describe a page at a known URL, and a 404 answers on every wrong URL there is.

The number on that page is set, never drawn. Constructing the digits out of strokes was tried, and out of a waveform after that, and both read as a wireframe standing next to Inter rather than as part of it. It is now the page's own typeface at its own tight tracking, one step larger than anything else on the site, with a vertical gradient for depth that stays on the mark's neutral axis.

The digits fade up in sequence over a third of a second. The global reduced-motion rule in `responsive.css` kills that animation, which is why that block also resets their opacity: without it the number would never appear at all.

## Hosting

The site is served by Cloudflare Workers Static Assets on the free plan, deployed by the `deploy` job in `.github/workflows/site.yml`. `wrangler.jsonc` is the whole deployment contract: `dist/` is uploaded as-is, there is no Worker code, and `public/_headers` travels with it as the response-header policy.

The custom domain is the only address the site answers on. `workers_dev` and `preview_urls` are both off, because a landing page exists to be found and a second origin serving the same HTML is what undermines that.

Deploys run on push to `main` and on `release: published`. The second trigger matters: the release badge in the hero is resolved at build time, so without a rebuild it keeps naming the previous version until someone edits the copy.

### One-time setup

These live in the Cloudflare and GitHub dashboards, not in this repository.

1. Create the Worker once — `pnpm dlx wrangler@4.134.0 deploy` from `site/`, or let the first CI run create it.
2. Attach the domain: **Workers & Pages → sotto-site → Settings → Domains & Routes → Add custom domain**. Cloudflare issues the certificate and writes the DNS record itself. `.app` is in the HSTS preload list, so the site is HTTPS-only from the first request.
3. Add `www` as a second custom domain, then redirect it: **Rules → Redirect Rules**, hostname equals `www.sotto.app`, 301 to `https://sotto.app` with the path preserved. `_redirects` cannot do this — it has no notion of hostnames — so the rule has to live at the zone level.
4. Create a scoped API token — **My Profile → API Tokens**, template *Edit Cloudflare Workers*, restricted to this account and zone — and store it as the `CLOUDFLARE_API_TOKEN` repository secret, with the account ID as `CLOUDFLARE_ACCOUNT_ID`. Both are read by the `production` environment, which is where the branch protection belongs.
5. Turn on analytics: **Web Analytics → Add a site → sotto.app**, automatic setup. Nothing is added to the page source — Cloudflare injects the beacon into the HTML on the way out. This is why `_headers` deliberately omits `no-transform`: that directive forbids exactly the rewrite the injection depends on.

### Content Security Policy

`public/_headers` sends `default-src 'none'` and opens three holes, each of which is load-bearing:

- `script-src` allows `static.cloudflareinsights.com`, the analytics beacon.
- `connect-src` allows `api.github.com`, which the download dialog queries for the latest release assets, and `cloudflareinsights.com`, where the beacon reports.
- `style-src` allows `'unsafe-inline'`, because the components set geometry through `style` attributes. Removing those attributes would let this one close.

Adding an external font, embed, or analytics script means widening this file, and forgetting to means the resource is silently blocked in the browser rather than at build time.

## Not yet decided

The canonical origin in `astro.config.mjs` is a placeholder. Set `SITE` to the real domain before the first deploy, and update `homepageUrl` on the GitHub repository to match.

The social preview image is language-neutral and shared by both locales. Per-locale images carrying each headline would read better when the page is shared.

The site is dark only. That was inherited from the draft rather than decided, and a light theme is now cheap to add: the mark is `currentColor` and the palette is a single neutral axis.

