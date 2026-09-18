/**
 * Derives the flat web glyph from the app-icon master.
 *
 * The identity exists only as raster artwork — `icon-source-1024.png` is the
 * master and there is no vector to export. A web mark has to scale down to
 * 20px and take the page's colour, so the ribbon is traced out of the master
 * and its rounded plate is dropped: the plate belongs in a dock, not beside a
 * line of text.
 *
 * The mark is a ribbon that folds, so a single silhouette would read as a blob.
 * Two shapes are traced instead — the whole ribbon, and the brighter outer face
 * on top of it — which keeps the fold legible with two tones of one colour and
 * no gradients.
 *
 * Re-run with `pnpm glyph` whenever the icon is redrawn.
 */
import { writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import sharp from 'sharp';
import potrace from 'potrace';

const root = new URL('../', import.meta.url);
const source = fileURLToPath(new URL('../desktop/src-tauri/icons/icon-source-1024.png', root));
const trace = promisify(potrace.trace);

/**
 * The master's luminance histogram has an empty band between 136 and 167: the
 * plate and the ribbon's shaded inner face sit below it, the lit outer face
 * above. Both cuts are taken from that gap rather than tuned by eye.
 */
const RIBBON = 70;
const LIT_FACE = 168;

const grey = sharp(source).flatten({ background: '#000000' }).greyscale();

const { data, info } = await grey.clone().raw().toBuffer({ resolveWithObject: true });

/**
 * The boundary between the two faces is a soft shading gradient, and a hard cut
 * across it frays. A small blur before that cut only — the ribbon's outline
 * against the plate is already crisp — keeps the fold edge smooth.
 */
const { data: softened } = await grey.clone().blur(3).raw().toBuffer({ resolveWithObject: true });

const { width, height } = info;

/**
 * The ribbon cut also catches the specular highlight along the plate's edge.
 * Keeping only the largest connected region drops it without guessing at
 * filter radii: the ribbon is by far the biggest bright shape in the mark.
 */
const largestRegion = (pixels) => {
  const labels = new Int32Array(width * height).fill(-1);
  const stack = [];
  let best = { label: -1, size: 0 };

  for (let start = 0; start < pixels.length; start += 1) {
    if (!pixels[start] || labels[start] !== -1) continue;

    const label = start;
    let size = 0;
    stack.push(start);
    labels[start] = label;

    while (stack.length) {
      const index = stack.pop();
      size += 1;
      const x = index % width;
      const y = (index / width) | 0;

      for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
        const nx = x + dx;
        const ny = y + dy;
        if (nx < 0 || ny < 0 || nx >= width || ny >= height) continue;
        const next = ny * width + nx;
        if (!pixels[next] || labels[next] !== -1) continue;
        labels[next] = label;
        stack.push(next);
      }
    }

    if (size > best.size) best = { label, size };
  }

  const out = Buffer.alloc(pixels.length, 0);
  for (let i = 0; i < pixels.length; i += 1) if (labels[i] === best.label) out[i] = 255;
  return { out, size: best.size };
};

const cut = (level) => {
  const binary = Buffer.alloc(data.length, 0);
  for (let i = 0; i < data.length; i += 1) binary[i] = data[i] > level ? 1 : 0;
  return binary;
};

const { out: ribbon, size: ribbonSize } = largestRegion(cut(RIBBON));

// The lit face is bounded by the ribbon, so it inherits the same cleanup.
const litFace = Buffer.alloc(data.length, 0);
for (let i = 0; i < data.length; i += 1) litFace[i] = ribbon[i] && softened[i] > LIT_FACE ? 255 : 0;

/** potrace traces dark shapes, so each mask is handed over inverted. */
const pathOf = async (mask) => {
  const png = await sharp(mask, { raw: { width, height, channels: 1 } }).negate().png().toBuffer();
  const svg = await trace(png, { threshold: 128, turdSize: 40, optCurve: true, optTolerance: 0.4 });
  const path = /<path[^>]*\sd="([^"]+)"/.exec(svg)?.[1];
  if (!path) throw new Error('potrace produced no path');
  return path;
};

const [ribbonPath, litPath] = await Promise.all([pathOf(ribbon), pathOf(litFace)]);

/**
 * The ribbon occupies only the middle band of the master, so the plate's empty
 * margins are cropped away. Without this the mark would sit small and lost
 * inside its own box wherever the page sizes it.
 */
const bounds = () => {
  let left = width;
  let top = height;
  let right = -1;
  let bottom = -1;

  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      if (!ribbon[y * width + x]) continue;
      if (x < left) left = x;
      if (x > right) right = x;
      if (y < top) top = y;
      if (y > bottom) bottom = y;
    }
  }

  const pad = Math.round((right - left) * 0.02);
  return {
    x: left - pad,
    y: top - pad,
    w: right - left + pad * 2,
    h: bottom - top + pad * 2,
  };
};

const box = bounds();

const glyph = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${box.x} ${box.y} ${box.w} ${box.h}" fill="currentColor">
  <path d="${ribbonPath}" opacity=".45"/>
  <path d="${litPath}"/>
</svg>
`;

await writeFile(fileURLToPath(new URL('src/assets/sotto-glyph.svg', root)), glyph);
console.log(
  `src/assets/sotto-glyph.svg — ${(glyph.length / 1024).toFixed(1)} KB, ` +
    `${box.w}×${box.h} (aspect ${(box.w / box.h).toFixed(2)}), ` +
    `ribbon covers ${((ribbonSize / (width * height)) * 100).toFixed(1)}% of the master`,
);
