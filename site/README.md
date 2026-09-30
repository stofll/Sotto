# Sotto website

The landing page for [Sotto](https://github.com/stofll/Sotto), built with Astro and served as static HTML.

It lives in this repository but installs separately: `site/` has its own `pnpm-lock.yaml` and is **not** a member of the `desktop/` workspace. Changing the landing page cannot touch the app's dependency graph, and a copy edit does not run the three-OS build matrix: `ui-tests.yml` ignores `site/**` outright, while `rust-ci.yml` filters per job. The difference matters, because every required status check on `main` lives in `rust-ci.yml` — a workflow that never starts reports no contexts at all, and a site-only pull request would wait on them forever, whereas a job skipped by `if:` counts as passed.

## Commands

```bash
pnpm install     # from this directory
pnpm dev         # http://localhost:4321
pnpm build       # static output in dist/
pnpm typecheck   # astro check
pnpm test        # after pnpm build: native catalog and generated search metadata
pnpm brand       # re-copy the icon from the desktop app
pnpm og          # regenerate the social preview images, one per locale
```

## Layout

`src/data/product.ts` holds facts that are not translated: links, model names, download sizes. Verify languages and sizes against [`desktop/src-tauri/src/model.rs`](../desktop/src-tauri/src/model.rs), which controls the actual downloads; [`docs/models.md`](../docs/models.md) explains the families. Whisper sizes use decimal MB/GB rounded from the shipped artifact, including quantization. The catalog tests detect drift from the native manifest.

`src/i18n/` holds everything a translator touches. `types.ts` is the contract: adding a string there makes `pnpm typecheck` fail for any locale that has not translated it yet, which is what keeps English and Russian from drifting apart. Russian terminology follows [`README.ru.md`](../README.ru.md) — «диктовка», «распознавание», «оверлей», «горячая клавиша».

## Brand and colour

`desktop/src-tauri/icons/icon-source-1024.png` is the only source of truth for the mark. `pnpm brand` copies the plated icon out of it twice: a 96 px PNG for the favicon and the social images, and a 56 px WebP for the header and the download dialog. Do not hand-place another copy: the site once drifted onto the previous icon exactly that way.

The page is dark only, on a warm near-black, with three steps each for backgrounds, text and borders in `src/styles/base.css`. One hue, `--accent` (orange), is kept for the download buttons, the second line of the headline and live state: the voice line, the caret and progress. Links in running text are underlined rather than coloured. Text colours clear WCAG AA against the page and the cards; `--text-muted` was lifted from the design draft's tone for exactly that reason.

## Download buttons

`src/scripts/platform.ts` reads the platform the browser reports, locally, and points the download buttons at the reader's own system: they open the setup dialog for Windows or macOS, and that platform's card in the Start section moves first. Apple Silicon and Intel Macs cannot be told apart reliably, so a Mac is offered the Apple Silicon build, and the setup dialog it opens says which build that is. On Linux, on a phone, or without JavaScript the buttons lead to the Start section with both platforms; phones also get a line saying Sotto is a desktop app.

## Type

Two self-hosted families: Unbounded for headings, as the static 500 cut every heading uses, and Onest for running text, as one variable file for its three weights. Labels use the system's monospace face, which costs nothing to load. Only the Latin and Cyrillic subsets are loaded. `Base.astro` preloads the Latin heading and body files on every page, since spaces, digits and product names come from them, and the Cyrillic ones on Russian pages.

Sizes are in rem and the root size is never pinned, so a reader who has enlarged their browser default gets a larger page. Page copy has a floor of 12px and labels a floor of 11px.

## Screenshots

The interface gallery uses dedicated, synthetic captures in `src/assets/screens/`. All three screens share a 1088×736 viewport, the same sidebar state and the same application build. Both themes and locales have native 1× and 2× lossless WebP captures; the gallery uses density-aware sources and the enlarged view uses the 2× original. Dark is the default, with a light-theme switch; do not resize older README screenshots to make them fit.

The nine-step walkthrough highlights features in the actual screenshots, with selectable numbered regions and explanations below the stationary window. Tabs, topic buttons and previous/next controls navigate manually. Automatic playback starts only on request and pauses outside the viewport, in a hidden tab, or on manual interaction. It stops at the last step. Incoming images are decoded before crossfading; loading failures preserve the current screen and allow retry. The hotspot coordinates in `src/data/screen-tour.ts` refer to the capture viewport and must be reviewed when app layout changes. Keyboard users can switch tabs with the arrow keys and close the enlarged view with Escape. Without JavaScript, all three screens and their original-image links remain available.

To refresh the captures, use the isolated browser harness described in [`docs/ui-testing.md`](../docs/ui-testing.md), with `SOTTO_SITE_SHOTS_DIR` pointing to a temporary output directory. Run `uv run --locked --project tests/ui pytest tests/ui/test_site_screenshots.py --browser chromium` from the repository root, review the 24 PNGs, then run `node scripts/import-screens.mjs <capture-directory>` from `site/`. This validates dimensions and encodes the originals without resampling. The capture fixture never opens native application data.

The model section introduces four examples; the full catalog and its filters live in a native disclosure that also works without JavaScript. Privacy details live on the dedicated localized page, linked from the header and footer, with a short answer in the FAQ.

For gallery, keyboard, zoom, model filtering, responsive layout and no-JavaScript regression checks, start `pnpm dev` or `pnpm preview`, set `SOTTO_SITE_URL` to its loopback URL, and run `uv run --locked --project tests/ui pytest site/tests/test_browser.py --browser chromium` from the repository root. Set `SOTTO_SITE_CHECKS_DIR` to a temporary directory for visual review captures. These checks cover both locales, both screenshot themes, system appearances, 1×/2× displays, normal and reduced motion, and image-loading failure/retry. The website itself uses a fixed dark theme.

`src/components/` renders the markup. Sections read their strings through `translationsFor(Astro.currentLocale)` rather than receiving them as props.

`src/scripts/` is progressive enhancement, one module per section. Nothing on the page requires it: with JavaScript disabled the copy, the links and the downloads all still work, the demos hold still, every screenshot is listed and the full model catalog can be expanded, and the controls that only filter or switch them are hidden. The voice animation runs only while one of its canvases is on screen, the tab is visible and reduced motion is off. The client scripts stay locale-agnostic by reading the strings that `RuntimeStrings.astro` renders into the page as JSON.

`src/styles/` is the stylesheet split by concern, imported in order by `global.css`. Load order matters: tokens first, breakpoints last. Astro inlines it into each page, which saves a render-blocking request.

The stylesheet describes the page as the scripts leave it — the menu button, the filters and the tabs are laid out from the first frame — so nothing shifts when the scripts load. What differs without JavaScript lives in one `<noscript>` block in `Base.astro`.

## Routing

English is served from `/`, Russian from `/ru/`. Adding a locale means adding it to `astro.config.mjs`, writing `src/i18n/<locale>.ts`, and creating `src/pages/<locale>/index.astro`.

Each locale also has a 404 page, and getting it to the right place takes one build step. Cloudflare answers a miss with the nearest file literally named `404.html`, walking up the tree — but Astro only special-cases the root `404.astro`, and emits a nested one as `dist/ru/404/index.html`, which that lookup cannot see. The `sotto:flatten-locale-404` integration in `astro.config.mjs` renames it to `dist/ru/404.html`, which is what makes `/ru/<anything-wrong>` answer in Russian instead of falling back to English. A new locale has to be listed there too.

Both 404 pages pass `noindex` to `Base.astro`, which drops the canonical link, the hreflang set and the structured data. All three describe a page at a known URL, and a 404 answers on every wrong URL there is.

The number on that page is set, never drawn. Constructing the digits out of strokes was tried, and out of a waveform after that, and both read as a wireframe standing next to the type rather than as part of it. It is the heading face at its own tight tracking, one step larger than anything else on the site, in the text colour.

The digits fade up in sequence over a third of a second. The reduced-motion block in `motion.css` kills that animation, which is why it also resets their opacity: without it the number would never appear at all.

## Search and AI assistants

The landing page's `<title>` and description carry the search terms, while the social card keeps the headline. Each locale gets its own social image from `pnpm og`, which draws the dictionary's headline with the site's fonts.

The FAQ section answers questions people ask before installing, with stable links to individual answers. Structured data connects the website, localized pages, app and FAQ; it describes actual content without promising rich results. `/llms.txt` and `/ru/llms.txt` provide optional text summaries from the same dictionaries and model catalog, with links back to the answers and documentation.

See [Search and agent discovery](SEARCH.md) for indexing checks, the Cloudflare and webmaster-console setup, measurement, and content priorities. `pnpm test` after a build checks both locales, canonical and alternate links, sitemap membership, social assets, FAQ parity, and the summaries. The site workflow also runs these checks when the native model manifest changes.

## Hosting

The site is served by Cloudflare Workers Static Assets on the free plan, deployed by the `deploy` job in `.github/workflows/site.yml`. `wrangler.jsonc` is the whole deployment contract: `dist/` is uploaded as-is, there is no Worker code, and `public/_headers` travels with it as the response-header policy.

The custom domain is the only address the site answers on. `workers_dev` and `preview_urls` are both off, because a landing page exists to be found and a second origin serving the same HTML is what undermines that.

Deploys run on push to `main` and on `release: published`. The second trigger matters: the release badge in the hero is resolved at build time, so without a rebuild it keeps naming the previous version until someone edits the copy.

### One-time setup

These live in the Cloudflare and GitHub dashboards, not in this repository.

1. Create the Worker once — `pnpm dlx wrangler@4.134.0 deploy` from `site/`, or let the first CI run create it.
2. Attach the domain: **Workers & Pages → sotto-site → Settings → Domains & Routes → Add custom domain**. Cloudflare issues the certificate and writes the DNS record itself. `.today` is not in the HSTS preload list, so turn on **SSL/TLS → Edge Certificates → Always Use HTTPS**: without it a first visit over plain HTTP is served as-is, before the `Strict-Transport-Security` header from `_headers` has taken effect.
3. Add `www` as a second custom domain, then redirect it: **Rules → Redirect Rules**, hostname equals `www.sotto.today`, 301 to `https://sotto.today` with the path preserved. `_redirects` cannot do this — it has no notion of hostnames — so the rule has to live at the zone level.
4. Create a scoped API token — **My Profile → API Tokens**, template *Edit Cloudflare Workers*, restricted to this account and zone — and store it as the `CLOUDFLARE_API_TOKEN` repository secret, with the account ID as `CLOUDFLARE_ACCOUNT_ID`. Both are read by the `production` environment, which is where the branch protection belongs.
5. Turn on analytics: **Web Analytics → Add a site → sotto.today**, automatic setup. Nothing is added to the page source — Cloudflare injects the beacon into the HTML on the way out. This is why `_headers` deliberately omits `no-transform`: that directive forbids exactly the rewrite the injection depends on.

### Content Security Policy

`public/_headers` sends `default-src 'none'` and opens three holes, each of which is load-bearing:

- `script-src` allows `static.cloudflareinsights.com`, the analytics beacon.
- `connect-src` allows `api.github.com`, which the download dialog queries for the latest release assets, and `cloudflareinsights.com`, where the beacon reports.
- `style-src` allows `'unsafe-inline'`, because the components set geometry through `style` attributes. Removing those attributes would let this one close.

Adding an external font, embed, or analytics script means widening this file, and forgetting to means the resource is silently blocked in the browser rather than at build time.
