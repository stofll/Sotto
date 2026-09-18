/**
 * Copies the brand mark out of the desktop app so the site cannot fall behind
 * it — which is exactly what had happened: the landing page still carried the
 * icon from before the identity was redrawn.
 *
 * `desktop/src-tauri/icons/icon-source-1024.png` is the source of truth. Run
 * `pnpm brand` after it changes, then `pnpm glyph` and `pnpm og`.
 *
 * Only the plated icon is copied here — the favicon and the social image need a
 * real URL and a raster. The mark used on the page itself is the flat glyph
 * that `pnpm glyph` traces, which is vector and needs no copy of the master.
 */
import { writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

const root = new URL('../', import.meta.url);
const source = fileURLToPath(new URL('../desktop/src-tauri/icons/icon-source-1024.png', root));
const out = (path) => fileURLToPath(new URL(path, root));

const icon = await sharp(source).resize(96, 96).png({ compressionLevel: 9 }).toBuffer();
await writeFile(out('public/brand/sotto-icon.png'), icon);

console.log(`public/brand/sotto-icon.png — ${(icon.length / 1024).toFixed(1)} KB`);
