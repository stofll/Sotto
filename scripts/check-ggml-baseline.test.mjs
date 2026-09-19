import { strict as assert } from 'node:assert';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const checker = fileURLToPath(new URL('./check-ggml-baseline.mjs', import.meta.url));
const toolchain = fileURLToPath(new URL('./ggml-baseline.cmake', import.meta.url));
const check = (root) => spawnSync(process.execPath, [checker, root], { encoding: 'utf8' });

test('CMake replaces stale native flags and the checker rejects incompatible artifacts', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'sotto-baseline-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const source = join(root, 'source');
  const target = join(root, 'target');
  // Exercise an explicit Cargo target triple as used by release builds.
  const build = join(target, 'x86_64-pc-windows-msvc', 'release', 'build', 'whisper-rs-sys-test', 'out', 'build');
  mkdirSync(source, { recursive: true });
  writeFileSync(join(source, 'CMakeLists.txt'),
    'cmake_minimum_required(VERSION 3.14)\nproject(BaselineProbe NONE)\n');
  const configured = spawnSync('cmake', [
    '-S', source, '-B', build, `-DCMAKE_TOOLCHAIN_FILE=${toolchain}`,
    '-DGGML_NATIVE:BOOL=ON', '-DGGML_AVX512:BOOL=ON', '-DGGML_AVX2:BOOL=OFF',
    '-DGGML_CPU_ARM_ARCH:STRING=armv8.6-a',
  ], { encoding: 'utf8' });
  assert.equal(configured.status, 0, configured.error?.message ?? configured.stdout + configured.stderr);
  const valid = check(target);
  assert.equal(valid.status, 0, valid.stderr);
  assert.match(valid.stdout, /Verified Whisper CPU baseline/);

  const cache = join(build, 'CMakeCache.txt');
  const original = readFileSync(cache, 'utf8');
  for (const flag of ['GGML_NATIVE', 'GGML_AVX512']) {
    writeFileSync(cache, original.replace(`${flag}:BOOL=OFF`, `${flag}:BOOL=ON`));
    const invalid = check(target);
    assert.notEqual(invalid.status, 0);
    assert.match(invalid.stderr, new RegExp(`expected ${flag}=OFF, got ON`));
  }
});

test('missing build artifacts cannot silently pass', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'sotto-baseline-empty-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const result = check(root);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /No compiled Whisper CMake cache found/);
});
