import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

const source = process.argv[2];
if (!source) throw new Error('Usage: node scripts/import-screens.mjs <capture-directory>');
const destination = fileURLToPath(new URL('../src/assets/screens/', import.meta.url));
await mkdir(destination, { recursive: true });
// The feature tour zooms into these captures, so only the dark 2× originals ship.
for (const locale of ['ru', 'en']) {
  for (const screen of ['models', 'history']) {
    const name = `${screen}-${locale}-dark@2x`;
    const image = sharp(resolve(source, `${name}.png`));
    const { width, height } = await image.metadata();
    if (width !== 1088 * 2 || height !== 736 * 2) throw new Error(`${name}: expected a 1088×736 capture at 2x`);
    await image.webp({ lossless: true, effort: 6 }).toFile(resolve(destination, `${name}.webp`));
  }
}
