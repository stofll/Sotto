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

## Not yet decided

The canonical origin in `astro.config.mjs` is a placeholder. Set `SITE` to the real domain before the first deploy, and update `homepageUrl` on the GitHub repository to match.

There is no deploy job yet, because the host has not been chosen. `.github/workflows/site.yml` currently only typechecks and builds on pull requests.

The social preview image is language-neutral and shared by both locales. Per-locale images carrying each headline would read better when the page is shared.

The site is dark only. That was inherited from the draft rather than decided, and a light theme is now cheap to add: the mark is `currentColor` and the palette is a single neutral axis.

The release badge in the hero is resolved at build time, so it is only as current as the last deploy. Deploying on `release: published` keeps it honest.
