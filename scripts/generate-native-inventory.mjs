import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { resolve } from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const digest = (text) => createHash('sha256').update(text).digest('hex');

export function nativeInventory(cargoLock, runtimeLock, components) {
  const packages = new Map(
    [...cargoLock.matchAll(/\[\[package\]\]\s+name = "([^"]+)"\s+version = "([^"]+)"/g)]
      .map((match) => [match[1], match[2]]),
  );
  if (packages.get(components.whisper.crate) !== components.whisper.crateVersion) {
    throw new Error('Update native-components.json for the locked Whisper source');
  }
  const rows = runtimeLock.split(/\r?\n/).map((row) => row.trim())
    .filter((row) => row && !row.startsWith('#')).map((row) => row.split(/\s+/));
  const version = rows.find(([key]) => key === 'version')?.[1];
  if (!version || version !== components.sherpaVersion
      || packages.get('sherpa-onnx-sys') !== version || packages.get('sherpa-onnx') !== version) {
    throw new Error('Sherpa Cargo, archive and native component versions disagree');
  }
  const archives = rows.filter(([key]) => key !== 'version').map(([target, file, sha256]) => {
    if (!/^[a-f0-9]{64}$/.test(sha256 ?? '') || !file.includes(`v${version}-`)) {
      throw new Error(`Invalid pinned native archive: ${target}`);
    }
    const onnxruntime = components.onnxruntime[target];
    if (!onnxruntime) throw new Error(`Missing ONNX Runtime inventory: ${target}`);
    return {
      target, file, sha256,
      url: `https://github.com/k2-fsa/sherpa-onnx/releases/download/v${version}/${file}`,
      components: [
        { name: 'sherpa-onnx', version, license: 'Apache-2.0' },
        { name: 'onnxruntime', ...onnxruntime },
      ],
    };
  });
  if (archives.length !== Object.keys(components.onnxruntime).length
      || new Set(archives.map((archive) => archive.target)).size !== archives.length) {
    throw new Error('Native archive targets and component inventory disagree');
  }
  return {
    formatVersion: 1,
    scope: 'Native source/archive inventory; not a scan of installed binaries. Packaging verifies archive SHA-256.',
    lockfiles: {
      'desktop/src-tauri/Cargo.lock': digest(cargoLock),
      'scripts/sherpa-runtime.lock': digest(runtimeLock),
    },
    vendored: [{ name: 'whisper.cpp', ...components.whisper }],
    archives,
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const output = process.argv[2];
  if (!output) throw new Error('Usage: node scripts/generate-native-inventory.mjs <output.json>');
  const inventory = nativeInventory(
    readFileSync(resolve(root, 'desktop/src-tauri/Cargo.lock'), 'utf8'),
    readFileSync(resolve(root, 'scripts/sherpa-runtime.lock'), 'utf8'),
    JSON.parse(readFileSync(resolve(root, 'scripts/native-components.json'), 'utf8')),
  );
  inventory.sourceCommit = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
  writeFileSync(output, `${JSON.stringify(inventory, null, 2)}\n`);
}
