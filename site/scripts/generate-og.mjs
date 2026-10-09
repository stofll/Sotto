/**
 * Renders the social preview images, one per locale, into public/og/.
 *
 * The copy comes from the dictionaries (Node strips the TypeScript types on
 * import), so a changed headline only needs `pnpm og` to reach the card. Text
 * is drawn as outlines with fontkit: sharp rasterizes SVG with whatever fonts
 * the system has, and the site's own faces are not among them.
 *
 * Run with `pnpm og` after changing the headline, the palette or the brand icon.
 */
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import * as fontkit from 'fontkit';
import sharp from 'sharp';
import { en } from '../src/i18n/en.ts';
import { ru } from '../src/i18n/ru.ts';

const WIDTH = 1200;
const HEIGHT = 630;
const COLORS = { bg: '#f2f3f0', text: '#14171a', secondary: '#4e555d', accent: '#2d4fd6' };

const root = new URL('../', import.meta.url);
const path = (relative) => fileURLToPath(new URL(relative, root));
/** Golos Text at one weight. fontkit cannot instance a variable WOFF2, so the card uses the static cuts. */
const font = async (file) => fontkit.create(await readFile(path(`node_modules/@fontsource/golos-text/files/${file}`)));

// The family ships as a Latin and a Cyrillic file; a character is drawn from
// whichever of the pair has it.
const display = [await font('golos-text-latin-600-normal.woff2'), await font('golos-text-cyrillic-600-normal.woff2')];
const body = [await font('golos-text-latin-400-normal.woff2'), await font('golos-text-cyrillic-400-normal.woff2')];

/** One line of text as a single SVG path, starting at (x, baseline). */
const outline = (fonts, text, x, baseline, size, tracking = 0) => {
  let pen = x;
  const parts = [];
  for (const char of text) {
    const codePoint = char.codePointAt(0);
    const face = fonts.find((candidate) => candidate.hasGlyphForCodePoint(codePoint)) ?? fonts[0];
    const scale = size / face.unitsPerEm;
    const glyph = face.glyphForCodePoint(codePoint);
    const commands = glyph.path.scale(scale, -scale).translate(pen, baseline).toSVG();
    if (commands) parts.push(commands);
    pen += glyph.advanceWidth * scale + tracking * size;
  }
  return { d: parts.join(''), width: pen - x };
};

/** Greedy word wrap against the measured width. */
const wrap = (fonts, text, size, maxWidth) => {
  const lines = [];
  let line = '';
  for (const word of text.split(' ')) {
    const candidate = line ? `${line} ${word}` : word;
    if (line && outline(fonts, candidate, 0, 0, size).width > maxWidth) {
      lines.push(line);
      line = word;
    } else {
      line = candidate;
    }
  }
  if (line) lines.push(line);
  return lines;
};

const icon = await readFile(path('public/brand/sotto-icon.png'));

// The hero's wallpaper as a band along the bottom. librsvg cannot read WebP, so it is passed as PNG.
const BAND = 150;
// The band is cut where the far ridges meet the mist, so it reads as a horizon rather than a dark stripe.
const wallpaper = await sharp(path('src/assets/wallpaper/hills-2400.webp'))
  .extract({ left: 0, top: 860, width: 2400, height: 300 }).resize(WIDTH, BAND).png().toBuffer();

const render = async (locale, t) => {
  const left = 88;
  const word = outline(display, 'sotto', left + 70, 122, 32, -0.02);
  const line1 = outline(display, t.hero.titleLine1, left, 246, 80, -0.04);
  const line2 = outline(display, t.hero.titleLine2, left, 330, 80, -0.04);
  const subtitle = wrap(body, t.meta.ogDescription, 28, WIDTH - left * 2)
    .map((text, index) => outline(body, text, left, 400 + index * 40, 28).d);

  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${WIDTH}" height="${HEIGHT}" viewBox="0 0 ${WIDTH} ${HEIGHT}">
  <rect width="${WIDTH}" height="${HEIGHT}" fill="${COLORS.bg}"/>
  <image href="data:image/png;base64,${icon.toString('base64')}" x="${left}" y="90" width="48" height="48"/>
  <path d="${word.d}" fill="${COLORS.text}"/>
  <path d="${line1.d}" fill="${COLORS.text}"/>
  <path d="${line2.d}" fill="${COLORS.accent}"/>
  <path d="${subtitle.join('')}" fill="${COLORS.secondary}"/>
  <image href="data:image/png;base64,${wallpaper.toString('base64')}" x="0" y="${HEIGHT - BAND}" width="${WIDTH}" height="${BAND}"/>
</svg>`;

  const png = await sharp(Buffer.from(svg)).png({ compressionLevel: 9 }).toBuffer();
  const file = `public/og/sotto-${locale}.png`;
  await writeFile(path(file), png);
  console.log(`${file} — ${(png.length / 1024).toFixed(1)} KB`);
};

await render('en', en);
await render('ru', ru);
