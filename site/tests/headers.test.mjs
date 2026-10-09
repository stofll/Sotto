import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { glob, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const dist = fileURLToPath(new URL('../dist/', import.meta.url));
const headers = await readFile(new URL('../public/_headers', import.meta.url), 'utf8');
const scriptSrc = headers.match(/script-src ([^;]+);/)?.[1].split(/\s+/) ?? [];

/** Script elements a browser runs: no `src`, and no data type such as JSON-LD. */
const inlineScripts = (html) => [...html.matchAll(/<script(?![^>]*\bsrc=)([^>]*)>([\s\S]*?)<\/script>/g)]
  .filter(([, attributes]) => !/\btype="application\/(?:ld\+)?json"/.test(attributes))
  .map(([, , body]) => body);

test('every inline script on every page is allowed by its hash', async () => {
  let seen = 0;
  for await (const file of glob('**/*.html', { cwd: dist })) {
    for (const body of inlineScripts(await readFile(join(dist, file), 'utf8'))) {
      const hash = `'sha256-${createHash('sha256').update(body).digest('base64')}'`;
      assert.ok(scriptSrc.includes(hash), `${file}: add ${hash} to script-src in public/_headers, or drop the inline script`);
      seen += 1;
    }
  }
  // The theme script is on every page; finding none means the pattern above has gone stale.
  assert.ok(seen > 0);
});
