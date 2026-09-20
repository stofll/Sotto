import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { nativeInventory } from './generate-native-inventory.mjs';

const cargo = readFileSync(new URL('../desktop/src-tauri/Cargo.lock', import.meta.url), 'utf8');
const runtime = readFileSync(new URL('./sherpa-runtime.lock', import.meta.url), 'utf8');
const components = JSON.parse(readFileSync(new URL('./native-components.json', import.meta.url), 'utf8'));

test('inventory includes each pinned archive and its platform-specific ONNX Runtime', () => {
  const inventory = nativeInventory(cargo, runtime, components);
  assert.equal(inventory.vendored[0].name, 'whisper.cpp');
  assert.equal(inventory.archives.length, 2);
  assert.equal(inventory.archives.find((a) => a.target === 'win-x64-shared').components[1].version, '1.27.1');
  assert.equal(inventory.archives.find((a) => a.target === 'osx-arm64-static').components[1].version, '1.28.1');
  assert.match(inventory.lockfiles['scripts/sherpa-runtime.lock'], /^[a-f0-9]{64}$/);
});

test('dependency updates require a reviewed native inventory', () => {
  const changed = structuredClone(components);
  changed.whisper.crateVersion = '999.0.0';
  assert.throws(() => nativeInventory(cargo, runtime, changed), /locked Whisper source/);
  changed.whisper.crateVersion = components.whisper.crateVersion;
  changed.sherpaVersion = '999.0.0';
  assert.throws(() => nativeInventory(cargo, runtime, changed), /versions disagree/);
});

test('missing target metadata and malformed hashes fail rather than emit an incomplete inventory', () => {
  const changed = structuredClone(components);
  delete changed.onnxruntime['win-x64-shared'];
  assert.throws(() => nativeInventory(cargo, runtime, changed), /Missing ONNX Runtime/);
  assert.throws(() => nativeInventory(cargo, runtime.replace(/[a-f0-9]{64}/, 'bad-hash'), components), /Invalid pinned/);
});
