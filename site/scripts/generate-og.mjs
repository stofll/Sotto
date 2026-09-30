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
const COLORS = { bg: '#0c0b0a', text: '#edeae4', secondary: '#a39b91', muted: '#867e74', accent: '#ff7a2f' };

const root = new URL('../', import.meta.url);
const path = (relative) => fileURLToPath(new URL(relative, root));
const font = async (file) => fontkit.create(await readFile(path(`node_modules/${file}`)));

// Each family ships as a Latin and a Cyrillic file; a character is drawn from
// whichever of the pair has it.
const display = [
  await font('@fontsource/unbounded/files/unbounded-latin-500-normal.woff2'),
  await font('@fontsource/unbounded/files/unbounded-cyrillic-500-normal.woff2'),
];
const body = [
  await font('@fontsource-variable/onest/files/onest-latin-wght-normal.woff2'),
  await font('@fontsource-variable/onest/files/onest-cyrillic-wght-normal.woff2'),
];

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

/** The voice line from the hero, still: seven detuned strands trailing into a dot. */
const wave = () => {
  const mid = 520;
  const split = WIDTH - 150;
  const strands = [];
  for (let strand = 0; strand < 7; strand += 1) {
    const points = [];
    for (let x = 0; x <= split; x += 6) {
      const u = x / split;
      const taper = Math.pow(u, 1.4) * 44;
      const y = mid + taper * (0.6 * Math.sin(x * 0.012 + 2.1 + strand * 1.1) + 0.4 * Math.sin(x * 0.031 - 3.4 + strand * 0.6));
      points.push(`${x},${y.toFixed(1)}`);
    }
    strands.push(`<polyline points="${points.join(' ')}" fill="none" stroke="url(#fade)" stroke-width="${strand ? 1.2 : 2.4}"/>`);
  }
  return `${strands.join('')}<circle cx="${split}" cy="${mid}" r="16" fill="${COLORS.accent}" fill-opacity=".22"/><circle cx="${split}" cy="${mid}" r="7" fill="${COLORS.accent}"/>`;
};

const render = async (locale, t) => {
  const left = 88;
  const word = outline(display, 'sotto', left + 76, 124, 34);
  const line1 = outline(display, t.hero.titleLine1, left, 258, 76, -0.035);
  const line2 = outline(display, t.hero.titleLine2, left, 350, 76, -0.035);
  const subtitle = wrap(body, t.meta.ogDescription, 30, WIDTH - left * 2)
    .map((text, index) => outline(body, text, left, 424 + index * 42, 30).d);

  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${WIDTH}" height="${HEIGHT}" viewBox="0 0 ${WIDTH} ${HEIGHT}">
  <defs>
    <linearGradient id="fade" x1="0" x2="1" y1="0" y2="0">
      <stop offset="0" stop-color="${COLORS.accent}" stop-opacity="0"/>
      <stop offset="1" stop-color="${COLORS.accent}" stop-opacity="1"/>
    </linearGradient>
  </defs>
  <rect width="${WIDTH}" height="${HEIGHT}" fill="${COLORS.bg}"/>
  <image href="data:image/png;base64,${icon.toString('base64')}" x="${left}" y="88" width="52" height="52"/>
  <path d="${word.d}" fill="${COLORS.text}"/>
  <path d="${line1.d}" fill="${COLORS.text}"/>
  <path d="${line2.d}" fill="${COLORS.accent}"/>
  <path d="${subtitle.join('')}" fill="${COLORS.secondary}"/>
  ${wave()}
</svg>`;

  const png = await sharp(Buffer.from(svg)).png({ compressionLevel: 9 }).toBuffer();
  const file = `public/og/sotto-${locale}.png`;
  await writeFile(path(file), png);
  console.log(`${file} — ${(png.length / 1024).toFixed(1)} KB`);
};

await render('en', en);
await render('ru', ru);
