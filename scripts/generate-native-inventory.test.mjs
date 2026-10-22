import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { nativeInventory } from './generate-native-inventory.mjs';

const cargo = readFileSync(new URL('../desktop/src-tauri/Cargo.lock', import.meta.url), 'utf8');
const runtime = readFileSync(new URL('./sherpa-runtime.lock', import.meta.url), 'utf8');
const components = JSON.parse(readFileSync(new URL('./native-components.json', import.meta.url), 'utf8'));
const sourceFiles = Object.fromEntries(Object.values(components.localPatches ?? {})
  .flatMap((patch) => [...patch.files, patch.provenance].map((file) => [`${patch.path}/${file}`, Buffer.from('source fixture\n')])));

const patchPath = 'desktop/src-tauri/vendor/whisper-rs-sys';
const patchedCargo = `[[package]]\nname = "whisper-rs-sys"\nversion = "${components.whisper.crateVersion}"\n\n[[package]]\nname = "sherpa-onnx-sys"\nversion = "${components.sherpaVersion}"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n\n[[package]]\nname = "sherpa-onnx"\nversion = "${components.sherpaVersion}"\n`;
const patch = {
  crateVersion: components.whisper.crateVersion,
  path: patchPath,
  source: components.whisper.source,
  sourceSha256: 'a'.repeat(64),
  description: 'Reject malformed model headers before allocation',
  provenance: 'SOTTO-PATCH.md',
  files: ['build.rs', 'whisper.cpp/src/whisper.cpp'],
};
const patchedComponents = { ...components, localPatches: { 'whisper-rs-sys': patch } };
const patchedFiles = {
  [`${patchPath}/build.rs`]: Buffer.from('fn main() {}\n'),
  [`${patchPath}/whisper.cpp/src/whisper.cpp`]: Buffer.from('checked_model_loader();\n'),
  [`${patchPath}/SOTTO-PATCH.md`]: Buffer.from('Upstream crate plus loader validation.\n'),
};

test('inventory includes each pinned archive and its platform-specific ONNX Runtime', () => {
  const inventory = nativeInventory(cargo, runtime, components, sourceFiles);
  assert.equal(inventory.vendored[0].name, 'whisper.cpp');
  assert.equal(inventory.archives.length, 2);
  assert.equal(inventory.archives.find((a) => a.target === 'win-x64-shared').components[1].version, '1.28.2');
  assert.equal(inventory.archives.find((a) => a.target === 'osx-arm64-static').components[1].version, '1.28.2');
  assert.match(inventory.lockfiles['scripts/sherpa-runtime.lock'], /^[a-f0-9]{64}$/);
});

test('dependency updates require a reviewed native inventory', () => {
  const changed = structuredClone(components);
  changed.whisper.crateVersion = '999.0.0';
  assert.throws(() => nativeInventory(cargo, runtime, changed, sourceFiles), /locked Whisper source/);
  changed.whisper.crateVersion = components.whisper.crateVersion;
  changed.sherpaVersion = '999.0.0';
  assert.throws(() => nativeInventory(cargo, runtime, changed, sourceFiles), /versions disagree/);
});

test('missing target metadata and malformed hashes fail rather than emit an incomplete inventory', () => {
  const changed = structuredClone(components);
  delete changed.onnxruntime['win-x64-shared'];
  assert.throws(() => nativeInventory(cargo, runtime, changed, sourceFiles), /Missing ONNX Runtime/);
  assert.throws(() => nativeInventory(cargo, runtime.replace(/[a-f0-9]{64}/, 'bad-hash'), components, sourceFiles), /Invalid pinned/);
});

test('local source patches retain upstream identity and fingerprint their actual code and provenance', () => {
  const inventory = nativeInventory(patchedCargo, runtime, patchedComponents, patchedFiles);
  const localPatch = inventory.localPatches[0];
  assert.equal(inventory.vendored[0].version, '1.7.4');
  assert.equal(localPatch.crate, 'whisper-rs-sys');
  assert.equal(localPatch.source, patch.source);
  assert.equal(localPatch.sourceSha256, patch.sourceSha256);
  assert.equal(localPatch.description, patch.description);
  for (const file of localPatch.files) {
    assert.equal(file.sha256, createHash('sha256').update(patchedFiles[file.path]).digest('hex'));
  }
  assert.deepEqual(localPatch.files.map((file) => file.path), Object.keys(patchedFiles));
  const changedFiles = { ...patchedFiles, [`${patchPath}/whisper.cpp/src/whisper.cpp`]: Buffer.from('updated_loader();\n') };
  const changed = nativeInventory(patchedCargo, runtime, patchedComponents, changedFiles);
  assert.notEqual(changed.localPatches[0].sha256, localPatch.sha256);
});

test('path dependencies require complete reviewed patch metadata and available source files', () => {
  assert.throws(() => nativeInventory(patchedCargo, runtime, { ...components, localPatches: {} }, patchedFiles), /Missing local patch metadata/);
  for (const crate of Object.keys(components.localPatches)) {
    const changed = structuredClone(components);
    delete changed.localPatches[crate];
    assert.throws(() => nativeInventory(cargo, runtime, changed, sourceFiles), /Missing local patch metadata/, crate);
  }
  for (const field of ['crateVersion', 'path', 'source', 'sourceSha256', 'description', 'provenance', 'files']) {
    const changed = structuredClone(patchedComponents);
    delete changed.localPatches['whisper-rs-sys'][field];
    assert.throws(() => nativeInventory(patchedCargo, runtime, changed, patchedFiles), /Invalid local patch metadata/, field);
  }
  const missingFile = { ...patchedFiles };
  delete missingFile[`${patchPath}/whisper.cpp/src/whisper.cpp`];
  assert.throws(() => nativeInventory(patchedCargo, runtime, patchedComponents, missingFile), /Missing local patch source/);
});

test('patch inventories reject unsafe paths, duplicate files and stale version or source claims', () => {
  for (const [field, value] of [
    ['path', '../../outside'],
    ['sourceSha256', 'invalid'],
    ['crateVersion', '999.0.0'],
    ['source', 'not a registry URL'],
    ['provenance', '../SOTTO-PATCH.md'],
    ['files', ['../outside.cpp']],
    ['files', ['build.rs', 'build.rs']],
    ['files', []],
  ]) {
    const changed = structuredClone(patchedComponents);
    changed.localPatches['whisper-rs-sys'][field] = value;
    assert.throws(() => nativeInventory(patchedCargo, runtime, changed, patchedFiles), /Invalid local patch metadata/, `${field}: ${value}`);
  }
  const registryCargo = patchedCargo.replace(`version = "${components.whisper.crateVersion}"`, `version = "${components.whisper.crateVersion}"\nsource = "registry+https://github.com/rust-lang/crates.io-index"`);
  assert.throws(() => nativeInventory(registryCargo, runtime, patchedComponents, patchedFiles), /Local patch is not selected/);
});
