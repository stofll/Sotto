/**
 * Copies the brand mark out of the desktop app so the site cannot fall behind
 * it — which is exactly what had happened: the landing page still carried the
 * icon from before the identity was redrawn.
 *
 * `desktop/src-tauri/icons/icon-source-1024.png` is the source of truth. Run
 * `pnpm brand` after it changes, then `pnpm og`.
 *
 * Two copies of the plated icon: a 96 px PNG for the favicon and the social
 * image, and a 56 px WebP for the header and the download dialog, which draw it
 * at 28 px and need no more than twice that on a high-density screen.
 */
import { writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

const root = new URL('../', import.meta.url);
const source = fileURLToPath(new URL('../desktop/src-tauri/icons/icon-source-1024.png', root));
const out = (path) => fileURLToPath(new URL(path, root));

const icon = await sharp(source).resize(96, 96).png({ compressionLevel: 9 }).toBuffer();
await writeFile(out('public/brand/sotto-icon.png'), icon);

const mark = await sharp(source).resize(56, 56).webp({ quality: 90 }).toBuffer();
await writeFile(out('public/brand/sotto-icon-56.webp'), mark);

console.log(`public/brand/sotto-icon.png — ${(icon.length / 1024).toFixed(1)} KB`);
console.log(`public/brand/sotto-icon-56.webp — ${(mark.length / 1024).toFixed(1)} KB`);
