import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { checkReleaseChanges } from './check-release-changes.mjs';

function git(root, ...args) {
  return execFileSync('git', ['-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', '-c', 'commit.gpgsign=false', ...args], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
}

function commit(root, path, text = 'Change') {
  mkdirSync(dirname(join(root, path)), { recursive: true });
  writeFileSync(join(root, path), text);
  git(root, 'add', '.');
  git(root, 'commit', '-qm', 'Fixture change');
  return git(root, 'rev-parse', 'HEAD');
}

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'sotto-release-changes-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  git(root, 'init', '-q', '-b', 'main');
  const source = commit(root, 'desktop/app.txt', 'Application');
  return { root, source };
}

test('prepares the first release and skips a source already tagged for a draft', (t) => {
  const { root, source } = fixture(t);
  assert.equal(checkReleaseChanges(root, source).changed, true);
  git(root, 'tag', '-a', 'v0.2.0', '-m', 'Draft release');
  assert.equal(checkReleaseChanges(root, source).changed, false);
});

test('skips site and documentation changes but includes application and build changes', (t) => {
  const { root } = fixture(t);
  git(root, 'tag', 'v0.2.0');
  for (const path of ['site/src/index.astro', 'site/package.json', 'docs/RELEASE.md', 'README.md', 'AGENTS.md']) {
    assert.equal(checkReleaseChanges(root, commit(root, path)).changed, false, path);
  }
  for (const path of ['desktop/src/app.ts', 'scripts/build.mjs', '.github/workflows/release.yml', '.cargo/config.toml', 'tests/ui/test_app.py']) {
    const source = commit(root, path);
    assert.equal(checkReleaseChanges(root, source).changed, true, path);
    git(root, 'tag', `v0.2.${git(root, 'rev-list', '--count', 'HEAD')}`);
  }
});

test('uses the greatest stable ancestor version and ignores other tags and branches', (t) => {
  const { root, source } = fixture(t);
  git(root, 'tag', 'v0.2.9');
  const release = commit(root, 'desktop/app.txt', 'Released application');
  git(root, 'tag', 'v0.2.10');
  git(root, 'tag', 'v9.0.0-rc.1');
  git(root, 'tag', 'v09.0.0');
  git(root, 'tag', 'other-v9.0.0');
  git(root, 'checkout', '-qb', 'unrelated', source);
  commit(root, 'desktop/app.txt', 'Unrelated application');
  git(root, 'tag', 'v10.0.0');
  git(root, 'checkout', '-q', 'main');
  assert.deepEqual(checkReleaseChanges(root, release), { changed: false, reason: 'No application or build changes since v0.2.10.' });
});

test('compares net committed changes at the pinned source and ignores working copy edits', (t) => {
  const { root, source } = fixture(t);
  git(root, 'tag', 'v0.2.0');
  commit(root, 'desktop/app.txt', 'Changed application');
  const reverted = commit(root, 'desktop/app.txt', 'Application');
  writeFileSync(join(root, 'desktop/app.txt'), 'Uncommitted work');
  assert.equal(checkReleaseChanges(root, source).changed, false);
  assert.equal(checkReleaseChanges(root, reverted).changed, false);
});

test('beta tags prevent duplicate preparations and a promoted stable tag wins over its betas', (t) => {
  const { root, source } = fixture(t);
  git(root, 'tag', 'v0.2.1-beta.1');
  assert.equal(checkReleaseChanges(root, source).changed, false);
  const beta = commit(root, 'desktop/app.txt', 'New beta');
  assert.equal(checkReleaseChanges(root, beta).changed, true);
  git(root, 'tag', 'v0.2.1-beta.2');
  assert.equal(checkReleaseChanges(root, beta).changed, false);
  const stable = commit(root, 'desktop/app.txt', 'Promoted stable');
  git(root, 'tag', 'v0.2.1');
  assert.deepEqual(checkReleaseChanges(root, stable), { changed: false, reason: 'No application or build changes since v0.2.1.' });
});

test('rejects invalid or unavailable source commits', (t) => {
  const { root } = fixture(t);
  for (const source of [undefined, '', 'main', '--all', 'a'.repeat(40)]) {
    assert.throws(() => checkReleaseChanges(root, source));
  }
});

test('CLI records the decision without modifying source files or tags', (t) => {
  const { root, source } = fixture(t);
  mkdirSync(join(root, 'scripts'));
  copyFileSync(fileURLToPath(new URL('./check-release-changes.mjs', import.meta.url)), join(root, 'scripts/check-release-changes.mjs'));
  copyFileSync(fileURLToPath(new URL('./release-version.mjs', import.meta.url)), join(root, 'scripts/release-version.mjs'));
  git(root, 'tag', 'v0.2.0');
  const output = join(root, 'outputs');
  const summary = join(root, 'summary');
  const result = spawnSync(process.execPath, ['scripts/check-release-changes.mjs'], {
    cwd: root, encoding: 'utf8',
    env: { ...process.env, RELEASE_SOURCE: source, GITHUB_OUTPUT: output, GITHUB_STEP_SUMMARY: summary },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(output, 'utf8'), 'changed=false\n');
  assert.match(readFileSync(summary, 'utf8'), /No application or build changes since v0.2.0/);
  assert.equal(git(root, 'rev-parse', 'HEAD'), source);
  assert.equal(git(root, 'tag', '--list'), 'v0.2.0');
  assert.equal(readFileSync(join(root, 'desktop/app.txt'), 'utf8'), 'Application');
});
