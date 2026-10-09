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

The palette is called Quiet: cool paper, ink and one pen blue by day, cool graphite with the same blue at night, with three steps each for backgrounds, text and borders in `src/styles/base.css`. Each token names its light and dark value once with `light-dark()`. It is chosen to sit apart from the orange-on-black look of developer tools, and its grey matches the overlay's default Graphite preset and the graphite icon. One hue, `--accent`, is kept for the download buttons, the second line of the headline, live state (the caret, the paste and progress) and the current step in the cleanup pipeline. The cleanup marks tell their steps apart by shape rather than by colour. Links in running text are underlined rather than coloured. Text colours clear WCAG AA against the page, the cards and the raised surface in both themes. On the dark page the button blue is too dim to read as text, so text and marks use `--accent-text`, which equals `--accent` by day.

The theme follows the system until the reader picks the other one with the switch in the header; a pick that matches the system is forgotten, so the page follows it again. The pick is kept in the browser's storage and applied by a one-line inline script in `<head>` before the first paint. The Content Security Policy in `public/_headers` allows that script by its hash, and `tests/headers.test.mjs` fails the build's tests when the two drift apart. A script that reads a colour token from CSS gets the `light-dark()` expression back, not a colour; read an element's computed `color` instead, as the voice lines do.

The overlay is the app's own, in its own colours rather than the site's tokens: the hero loads it from `desktop/src/overlay/`, and the stand-in pill that precedes it is drawn in the Graphite preset of `overlayPalette.ts`. The messenger on the hero's desktop draws another app, so its greys are its own.

The hero's desktop and the features stage share one wallpaper: misty hills in `src/assets/wallpaper/`, drawn by `scripts/wallpaper.html` rather than photographed, so it carries no licence and matches the palette. To redraw it, open that file in a browser, call `draw(2400, 1500, 4242)`, save the PNG and encode it to WebP at 2400 and 1200 pixels wide with quality 78. `pnpm og` reuses the large file for the social images.

## Download buttons

`src/scripts/platform.ts` reads the platform the browser reports, locally, and points the download buttons at the reader's own system: they open the setup dialog for Windows or macOS, and that platform's card in the Start section moves first. Apple Silicon and Intel Macs cannot be told apart reliably, so a Mac is offered the Apple Silicon build, and the setup dialog it opens says which build that is. On Linux, on a phone, or without JavaScript the buttons lead to the Start section with both platforms; phones also get a line saying Sotto is a desktop app.

## Type

One self-hosted family, Golos Text, sets headings and running text from one variable file per subset; headings differ by weight and tracking rather than by a second face. Labels use the system's monospace face, which costs nothing to load. Only the Latin and Cyrillic subsets are loaded. `Base.astro` preloads the Latin file on every page, since spaces, digits and product names come from it, and the Cyrillic one on Russian pages. `pnpm og` draws the social images from the static 400 and 600 cuts of the same family, because fontkit cannot instance a variable WOFF2.

Sizes are in rem and the root size is never pinned, so a reader who has enlarged their browser default gets a larger page. Page copy has a floor of 12px and labels a floor of 11px.

## Page story

The page shows Sotto in use before it shows its settings. Three demonstrations come first: the hero's desktop, cleanup close up, and the features stage. The facts strip and the app band follow, then the models for readers who want depth.

The hero's desktop is drawn in HTML and CSS at real scale rather than recorded, so it reads its words from the locale and stays sharp at any density. It wears the reader's own system, Windows or macOS, as `src/scripts/platform.ts` detects it, and Windows otherwise. It plays the app's real sequence in a messenger: the default hotkey from `desktop/src-tauri/src/config.rs` starts a recording, the overlay streams the raw words, a second press stops it, and the cleaned text lands in the field. A caption names each of the three steps.

The overlay on that desktop is the app's own rather than a copy, because overlay customisation is a feature the page has to show and a copy would drift. Once the desk is in view, `src/scripts/app-overlay/island.ts` loads the app's overlay scene from `desktop/src/overlay/` with Preact standing in for React; the `sotto:app-overlay` plugin in `astro.config.mjs` resolves the app's imports against the site's own install and replaces the Tauri bridge and the app's dictionaries with stand-ins, so the site needs neither the app's `node_modules` nor its English dictionary. The island and its styles load in the background, about 25 KB compressed, plus the Glow look's beam when it is first shown. Until it arrives, and without JavaScript, a stand-in pill drawn in plain HTML and CSS holds its place. A choice beneath the desk then shows the looks in turn or holds one, words while speaking or only after, and the four palettes. The site workflow runs on changes to the app files the overlay is built from.

Cleanup, close up, sets one sentence large. The raw words arrive while the voice line moves, then filler removal, dictionaries, replacements and finishing each change their own words, in the order the pipeline lists them. Its sample is the same one the features stage shows in its cleanup window.

The app band names places Sotto pastes into, not integrations, and its note says so. The marks come from Simple Icons and are listed in `src/data/app-logos.ts`; brands missing from Simple Icons are left out rather than drawn by hand. `src/pages/app-logos.svg.ts` emits them as one cached sprite, because the band repeats every mark for its loop.

The facts strip reuses the FAQ's facts.

## Features

The features share one stage on the wallpaper: a glass list on the left and one window at a time on the right, for voice, cleanup, models, the LLM profile, history and file transcription. The list walks through them by itself while the stage is on screen, until the reader points at, focuses or picks one. Panels overlap in one grid cell, so switching never moves the page.

Every window shows something the app actually does, drawn from the app's own names and limits. The models window reads names, engines, languages and sizes from `src/data/product.ts`, the profile window lists the same providers as the models section, and the file window lists the formats from `src/data/capabilities.ts`, which names the app source it mirrors. When the app changes a feature, its window changes with it.

The model section introduces four examples; the full catalog and its filters live in a native disclosure that also works without JavaScript. Privacy details live on the dedicated localized page, linked from the header and footer, with a short answer in the FAQ.

For the demonstrations, the app band, keyboard, model filtering, responsive layout and no-JavaScript regression checks, start `pnpm dev` or `pnpm preview`, set `SOTTO_SITE_URL` to its loopback URL, and run `uv run --locked --project tests/ui pytest tests/ui/test_site_browser.py --browser chromium` from the repository root. Set `SOTTO_SITE_CHECKS_DIR` to a temporary directory for visual review captures. These checks cover both locales, system appearances, phone and desktop widths, and normal and reduced motion. The captures cover both themes.

`src/components/` renders the markup. Sections read their strings through `translationsFor(Astro.currentLocale)` rather than receiving them as props.

`src/scripts/` is progressive enhancement, one module per section. Nothing on the page requires it: with JavaScript disabled the copy, the links and the downloads all still work, the desktop holds the moment after the paste, cleanup shows the cleaned sentence, every feature window is listed, the full model catalog can be expanded, and the controls that only switch or filter them are hidden. The demonstrations, the app band and the voice lines move only while on screen, in a visible tab and with reduced motion off. The client scripts stay locale-agnostic by reading the strings that `RuntimeStrings.astro` renders into the page as JSON or the text the page already shows.

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

The site is served by Cloudflare Workers Static Assets on the free plan, deployed by the `deploy` job in `.github/workflows/site.yml`. `wrangler.jsonc` is the whole deployment contract: `dist/` is uploaded as-is, there is no Worker code, and `public/_headers` travels with it as the response-header policy. `public/_redirects` answers the conventional `/sitemap.xml` and `/favicon.ico` that crawlers and browsers request unprompted, so those requests reach the real files instead of a 404.

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

- `script-src` allows `static.cloudflareinsights.com`, the analytics beacon, and the hash of the inline theme script from `src/scripts/theme.ts`, which runs before paint; editing that script means updating the hash.
- `connect-src` allows `api.github.com`, which the download dialog queries for the latest release assets, and `cloudflareinsights.com`, where the beacon reports.
- `style-src` allows `'unsafe-inline'`, because the components set geometry through `style` attributes and the app's overlay island injects `<style>` elements at runtime.

Adding an external font, embed, or analytics script means widening this file, and forgetting to means the resource is silently blocked in the browser rather than at build time.
