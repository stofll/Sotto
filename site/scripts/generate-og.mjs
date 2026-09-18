/**
 * Rasterizes src/assets/og.svg into the social preview PNG.
 *
 * The image is deliberately language-neutral: both locales share it, so it
 * carries the mark and the product category rather than a translated headline.
 * Run with `pnpm og` after changing the artwork or the brand icon.
 */
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

const root = new URL('../', import.meta.url);
const read = (path) => readFile(fileURLToPath(new URL(path, root)));

const icon = await read('public/brand/sotto-icon.png');
const template = await readFile(fileURLToPath(new URL('src/assets/og.svg', root)), 'utf8');

// The waveform echoes the overlay in the hero: same shape, calmer amplitude.
const heights = [20, 38, 57, 32, 76, 101, 58, 118, 82, 134, 93, 63, 123, 150];
const bars = heights
  .map((height, index) => {
    const x = 96 + index * 34;
    const y = 438 - height / 2;
    return `<rect x="${x}" y="${y.toFixed(1)}" width="10" height="${height}" rx="5"/>`;
  })
  .join('\n    ');

const svg = template
  .replace('ICON_DATA_URI', `data:image/png;base64,${icon.toString('base64')}`)
  .replace('WAVEFORM_BARS', bars);

const png = await sharp(Buffer.from(svg)).png({ compressionLevel: 9 }).toBuffer();
await writeFile(fileURLToPath(new URL('public/og/sotto.png', root)), png);

console.log(`public/og/sotto.png — ${(png.length / 1024).toFixed(1)} KB`);
