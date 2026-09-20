import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const script = fileURLToPath(new URL('./build-portable.ps1', import.meta.url));

for (const failArchive of [false, true]) {
  test(`portable staging is removed after ${failArchive ? 'archive failure' : 'success'}`, { skip: process.platform !== 'win32' }, (t) => {
    const tempRoot = resolve(tmpdir());
    const fixture = mkdtempSync(join(tempRoot, 'sotto-packaging-test-'));
    t.after(() => {
      assert.equal(dirname(fixture), tempRoot);
      rmSync(fixture, { recursive: true, force: true });
    });
    const binary = join(fixture, 'binary');
    const scratch = join(fixture, 'scratch');
    mkdirSync(binary);
    mkdirSync(scratch);
    for (const name of ['Sotto.exe', 'onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll']) {
      writeFileSync(join(binary, name), `synthetic ${name}`);
    }
    const output = failArchive ? join(fixture, 'not-a-directory', 'portable.zip') : join(fixture, 'portable.zip');
    if (failArchive) writeFileSync(join(fixture, 'not-a-directory'), 'blocks archive directory');
    const result = spawnSync('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script, '-BinaryDirectory', binary, '-OutputPath', output], {
      encoding: 'utf8', env: { ...process.env, TEMP: scratch, TMP: scratch },
    });
    if (failArchive) assert.notEqual(result.status, 0);
    else assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(readdirSync(scratch), [], 'temporary package contents must not remain');
    if (!failArchive) assert.ok(readdirSync(fixture).includes('portable.zip'));
  });
}
