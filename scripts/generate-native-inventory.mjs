import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { resolve } from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const digest = (text) => createHash('sha256').update(text).digest('hex');
const patchedCrates = ['whisper-rs-sys', 'sherpa-onnx-sys'];

function patchFiles(crate, patch) {
  const relativeFile = (file) => typeof file === 'string'
    && file.split('/').every((part) => /^[\w.-]+$/.test(part) && part !== '.' && part !== '..');
  if (!patchedCrates.includes(crate) || !patch || typeof patch.crateVersion !== 'string'
      || patch.path !== `desktop/src-tauri/vendor/${crate}`
      || patch.source !== `https://crates.io/crates/${crate}/${patch.crateVersion}`
      || !/^[a-f0-9]{64}$/.test(patch.sourceSha256 ?? '')
      || typeof patch.description !== 'string' || !patch.description.trim()
      || !relativeFile(patch.provenance) || !Array.isArray(patch.files) || !patch.files.length
      || !patch.files.every(relativeFile)
      || new Set([...patch.files, patch.provenance]).size !== patch.files.length + 1) {
    throw new Error(`Invalid local patch metadata: ${crate}`);
  }
  return [...patch.files, patch.provenance].map((file) => `${patch.path}/${file}`);
}

export function nativeInventory(cargoLock, runtimeLock, components, sourceFiles = {}) {
  const packages = new Map(
    [...cargoLock.matchAll(/\[\[package\]\]\s+name = "([^"]+)"\s+version = "([^"]+)"(?:\s+source = "([^"]+)")?/g)]
      .map((match) => [match[1], { version: match[2], source: match[3] }]),
  );
  if (packages.get(components.whisper.crate)?.version !== components.whisper.crateVersion) {
    throw new Error('Update native-components.json for the locked Whisper source');
  }
  const rows = runtimeLock.split(/\r?\n/).map((row) => row.trim())
    .filter((row) => row && !row.startsWith('#')).map((row) => row.split(/\s+/));
  const version = rows.find(([key]) => key === 'version')?.[1];
  if (!version || version !== components.sherpaVersion
      || packages.get('sherpa-onnx-sys')?.version !== version || packages.get('sherpa-onnx')?.version !== version) {
    throw new Error('Sherpa Cargo, archive and native component versions disagree');
  }
  const localPatches = [];
  for (const crate of new Set([...patchedCrates, ...Object.keys(components.localPatches ?? {})])) {
    const patch = components.localPatches?.[crate];
    if (packages.get(crate)?.source) {
      if (patch) throw new Error(`Local patch is not selected by Cargo.lock: ${crate}`);
      continue;
    }
    if (!patch) throw new Error(`Missing local patch metadata: ${crate}`);
    const files = patchFiles(crate, patch).map((path) => {
      if (sourceFiles[path] === undefined) throw new Error(`Missing local patch source: ${path}`);
      return { path, sha256: digest(sourceFiles[path]) };
    });
    if (patch.crateVersion !== packages.get(crate)?.version) {
      throw new Error(`Invalid local patch metadata version: ${crate}`);
    }
    localPatches.push({ ...patch, crate, files, sha256: digest(JSON.stringify(files)) });
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
    scope: 'Native source/archive inventory; not a scan of installed binaries. Packaging verifies archive SHA-256. Local patch hashes cover the listed modified files and provenance.',
    lockfiles: {
      'desktop/src-tauri/Cargo.lock': digest(cargoLock),
      'scripts/sherpa-runtime.lock': digest(runtimeLock),
    },
    vendored: [{ name: 'whisper.cpp', ...components.whisper }],
    localPatches,
    archives,
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const output = process.argv[2];
  if (!output) throw new Error('Usage: node scripts/generate-native-inventory.mjs <output.json>');
  const components = JSON.parse(readFileSync(resolve(root, 'scripts/native-components.json'), 'utf8'));
  const sourceFiles = Object.fromEntries(Object.entries(components.localPatches ?? {})
    .flatMap(([crate, patch]) => patchFiles(crate, patch))
    .map((path) => [path, readFileSync(resolve(root, path))]));
  const inventory = nativeInventory(
    readFileSync(resolve(root, 'desktop/src-tauri/Cargo.lock'), 'utf8'),
    readFileSync(resolve(root, 'scripts/sherpa-runtime.lock'), 'utf8'),
    components,
    sourceFiles,
  );
  inventory.sourceCommit = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
  writeFileSync(output, `${JSON.stringify(inventory, null, 2)}\n`);
}
