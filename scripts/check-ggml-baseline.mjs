// Inspect generated CMake caches, not just the requested toolchain settings.
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const root = process.argv[2];
if (!root) throw new Error('Usage: node scripts/check-ggml-baseline.mjs <cargo-target-dir>');

const expected = {
  GGML_NATIVE: 'OFF',
  GGML_AVX: 'ON',
  GGML_AVX2: 'ON',
  GGML_FMA: 'ON',
  GGML_F16C: 'ON',
  GGML_AVX_VNNI: 'OFF',
  GGML_AVX512: 'OFF',
  GGML_AVX512_VBMI: 'OFF',
  GGML_AVX512_VNNI: 'OFF',
  GGML_AVX512_BF16: 'OFF',
  GGML_AMX_TILE: 'OFF',
  GGML_AMX_INT8: 'OFF',
  GGML_AMX_BF16: 'OFF',
  GGML_CPU_ARM_ARCH: '',
};

let checked = 0;
function inspect(directory, depth = 0) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const path = join(directory, entry.name);
    if (entry.name === 'build') {
      for (const build of readdirSync(path, { withFileTypes: true })) {
        if (!build.isDirectory() || !build.name.startsWith('whisper-rs-sys-')) continue;
        const cache = join(path, build.name, 'out', 'build', 'CMakeCache.txt');
        let text;
        try {
          text = readFileSync(cache, 'utf8');
        } catch (error) {
          if (error.code === 'ENOENT') continue; // Cargo's build-script executable directory.
          throw error;
        }
        const values = new Map([...text.matchAll(/^([^#/:=\r\n]+):[^=\r\n]+=(.*)\r?$/gm)]
          .map(([, key, value]) => [key, value.trim()]));
        for (const [key, value] of Object.entries(expected)) {
          if (values.get(key) !== value) {
            throw new Error(`${cache}: expected ${key}=${value}, got ${values.get(key) ?? '<missing>'}`);
          }
        }
        console.log(`Verified Whisper CPU baseline: ${cache}`);
        checked++;
      }
    } else if (depth < 2 && !['deps', 'incremental', '.fingerprint'].includes(entry.name)) {
      inspect(path, depth + 1);
    }
  }
}
inspect(root);
if (!checked) throw new Error(`No compiled Whisper CMake cache found under ${root}`);
