import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

const source = process.argv[2];
if (!source) throw new Error('Usage: node scripts/import-screens.mjs <capture-directory>');
const destination = fileURLToPath(new URL('../src/assets/screens/', import.meta.url));
await mkdir(destination, { recursive: true });
for (const locale of ['ru', 'en']) {
  for (const screen of ['settings', 'models', 'history']) {
    for (const theme of ['dark', 'light']) {
      for (const density of [1, 2]) {
        const name = `${screen}-${locale}-${theme}${density === 2 ? '@2x' : ''}`;
        const image = sharp(resolve(source, `${name}.png`));
        const { width, height } = await image.metadata();
        if (width !== 1088 * density || height !== 736 * density) throw new Error(`${name}: expected a 1088×736 capture at ${density}x`);
        await image.webp({ lossless: true, effort: 6 }).toFile(resolve(destination, `${name}.webp`));
      }
    }
  }
}
