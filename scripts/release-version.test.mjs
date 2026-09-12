import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { nextVersion, updateVersions, versionFiles } from './release-version.mjs';

const repo = fileURLToPath(new URL('../', import.meta.url));

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'sotto-release-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const path of [...versionFiles, 'scripts/release-version.mjs', 'scripts/check-version.sh']) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    copyFileSync(join(repo, path), join(root, path));
  }
  updateVersions(root, '0.0.5');
  return root;
}

function snapshot(root) {
  return versionFiles.map((path) => readFileSync(join(root, path), 'utf8'));
}

test('patch, minor, and major reset the appropriate version components', () => {
  assert.equal(nextVersion('0.0.5', [], 'patch'), '0.0.6');
  assert.equal(nextVersion('0.2.5', [], 'minor'), '0.3.0');
  assert.equal(nextVersion('0.2.5', [], 'major'), '1.0.0');
});

test('uses the greatest stable tag or current version, including reserved draft tags', () => {
  const tags = ['v0.0.9', 'v0.0.10', 'v0.0.6', 'mobile-v9.0.0', 'v2.0.0-rc.1', 'v01.0.0'];
  assert.equal(nextVersion('0.0.5', tags, 'patch'), '0.0.11');
  assert.equal(nextVersion('0.1.0', tags, 'patch'), '0.1.1');
});

test('an exact stable version overrides the bump kind', () => {
  assert.equal(nextVersion('0.0.5', ['v0.0.6'], 'patch', ' 0.2.0 '), '0.2.0');
});

test('rejects reused versions, downgrades, prereleases, and malformed input', () => {
  for (const exact of ['0.0.5', '0.0.4', '0.0.6', 'v0.2.0', '01.0.0', '1.0', '1.0.0-rc.1', '1.0.0+build', '1.0.0\ntag=bad']) {
    assert.throws(() => nextVersion('0.0.5', ['v0.0.6'], 'patch', exact));
  }
  assert.throws(() => nextVersion('0.0.5', [], 'invalid'));
  assert.throws(() => nextVersion('0.0.5-rc.1', [], 'patch'));
});

test('updates all four sources, preserving every other byte and dependency version', (t) => {
  const root = fixture(t);
  const before = snapshot(root);
  updateVersions(root, '0.1.0');
  const after = snapshot(root);
  for (let i = 0; i < before.length; i++) {
    assert.notEqual(before[i], after[i]);
    // The old version may also occur in dependency entries; reversing only
    // the changed app version must reproduce the entire original file.
    let prefix = 0;
    while (before[i][prefix] === after[i][prefix]) prefix++;
    let suffix = 0;
    while (before[i].at(-suffix - 1) === after[i].at(-suffix - 1)) suffix++;
    assert.equal(before[i].slice(prefix, -suffix), '0.5');
    assert.equal(after[i].slice(prefix, -suffix), '1.0');
  }
  execFileSync('sh', ['scripts/check-version.sh', 'v0.1.0'], { cwd: root });
  assert.notEqual(spawnSync('sh', ['scripts/check-version.sh', 'v0.0.5'], { cwd: root }).status, 0);
});

test('mismatched or missing source fields fail before writing any file', (t) => {
  for (const corrupt of [
    (root) => {
      const path = join(root, versionFiles[3]);
      writeFileSync(path, readFileSync(path, 'utf8').replace('<string>0.0.5</string>', '<string>0.0.4</string>'));
    },
    (root) => {
      const path = join(root, versionFiles[1]);
      writeFileSync(path, readFileSync(path, 'utf8').replace('name = "sotto"', 'name = "missing"'));
    },
  ]) {
    const root = fixture(t);
    corrupt(root);
    const before = snapshot(root);
    assert.throws(() => updateVersions(root, '0.1.0'));
    assert.deepEqual(snapshot(root), before);
  }
});

test('repeated synchronization is idempotent', (t) => {
  const root = fixture(t);
  updateVersions(root, '0.1.0');
  const before = snapshot(root);
  updateVersions(root, '0.1.0');
  assert.deepEqual(snapshot(root), before);
});

test('CLI uses fetched tags and writes the selected version to GitHub outputs', (t) => {
  const root = fixture(t);
  execFileSync('git', ['init', '-q'], { cwd: root });
  execFileSync('git', ['add', '.'], { cwd: root });
  execFileSync('git', ['-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', '-c', 'commit.gpgsign=false', 'commit', '-qm', 'Fixture'], { cwd: root });
  execFileSync('git', ['tag', 'v0.0.7'], { cwd: root });
  const output = join(root, 'outputs');
  const result = spawnSync(process.execPath, ['scripts/release-version.mjs'], {
    cwd: root,
    env: { ...process.env, RELEASE_KIND: 'patch', RELEASE_VERSION: '', GITHUB_OUTPUT: output },
    encoding: 'utf8',
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(output, 'utf8'), 'version=0.0.8\ntag=v0.0.8\n');
  execFileSync('sh', ['scripts/check-version.sh', 'v0.0.8'], { cwd: root });
  const before = snapshot(root);
  const invalid = spawnSync(process.execPath, ['scripts/release-version.mjs'], {
    cwd: root,
    env: { ...process.env, RELEASE_KIND: 'patch', RELEASE_VERSION: '0.0.7', GITHUB_OUTPUT: output },
  });
  assert.notEqual(invalid.status, 0);
  assert.deepEqual(snapshot(root), before);
  assert.equal(readFileSync(output, 'utf8'), 'version=0.0.8\ntag=v0.0.8\n');
});
