import { build } from '../../../desktop/node_modules/esbuild/lib/main.js';
import { fileURLToPath } from 'node:url';
await build({
  entryPoints: [fileURLToPath(new URL('./runtime.ts', import.meta.url))],
  bundle: true,
  format: 'iife',
  globalName: 'SottoHarness',
  outfile: process.argv[2],
  nodePaths: [fileURLToPath(new URL('../../../desktop/node_modules', import.meta.url))],
});
