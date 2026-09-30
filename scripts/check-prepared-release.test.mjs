import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import { checkPreparedRelease } from './check-prepared-release.mjs';
import { updateVersions, versionFiles } from './release-version.mjs';

const repo = 'owner/repo';
const checkout = fileURLToPath(new URL('../', import.meta.url));

function git(root, ...args) {
  return execFileSync('git', ['-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', '-c', 'commit.gpgsign=false', ...args], {
    cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
}

function fixture(t, { marker = 'Release-Preparation: 123', version = '0.1.0' } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'sotto-prepared-release-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const path of versionFiles) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    copyFileSync(join(checkout, path), join(root, path));
  }
  updateVersions(root, '0.0.5');
  git(root, 'init', '-q');
  git(root, 'add', '.');
  git(root, 'commit', '-qm', 'Source');
  const source = git(root, 'rev-parse', 'HEAD');
  updateVersions(root, version);
  git(root, 'add', '.');
  git(root, 'commit', '-qm', `chore(release): prepare v${version}\n\n${marker}`);
  const run = {
    id: 123, run_attempt: 1, path: '.github/workflows/prepare-release.yml',
    repository: { full_name: repo }, head_repository: { full_name: repo },
    head_branch: 'main', head_sha: source, event: 'workflow_dispatch', status: 'completed', conclusion: 'success',
  };
  const job = { name: 'Commit and tag release version', status: 'completed', conclusion: 'success' };
  const requests = [];
  let waits = 0;
  const check = (options = {}) => checkPreparedRelease({
    root, repo, log: () => {}, wait: async () => { waits++; },
    api: async (endpoint) => {
      requests.push(endpoint);
      return endpoint.includes('/jobs?') ? [{ jobs: [job] }] : [run];
    }, ...options,
  });
  return { root, source, run, job, requests, check, waits: () => waits };
}

test('reuses successful preparation for stable and beta tags and the exact version push', async (t) => {
  for (const version of ['0.1.0', '0.1.0-beta.1']) {
    const f = fixture(t, { version });
    assert.equal(await f.check(), true);
    assert.equal(await f.check({ before: f.source }), true);
    assert.equal(await f.check({ before: 'a'.repeat(40) }), false);
    assert.equal(await f.check({ before: '' }), false);
    f.run.event = 'schedule';
    assert.equal(await f.check(), true);
  }
});

test('ordinary commits and legacy tags keep checks without querying Actions', async (t) => {
  for (const marker of ['', 'Release-Preparation: invalid', 'Release-Preparation: 123\nRelease-Preparation: 456']) {
    const f = fixture(t, { marker });
    assert.equal(await f.check(), false);
    assert.deepEqual(f.requests, []);
  }
});

test('verifies the two-commit shallow checkout used by both workflows', async (t) => {
  const f = fixture(t);
  const shallow = join(f.root, 'shallow');
  git(f.root, 'clone', '--depth=2', pathToFileURL(f.root).href, shallow);
  assert.equal(git(shallow, 'rev-parse', '--is-shallow-repository'), 'true');
  assert.equal(await f.check({ root: shallow, before: f.source }), true);
});

test('CLI keeps checks for a legacy tag and appends its decision without modifying Git', (t) => {
  const f = fixture(t, { marker: '' });
  const output = join(f.root, 'outputs');
  const head = git(f.root, 'rev-parse', 'HEAD');
  writeFileSync(output, 'existing=value\n');
  execFileSync(process.execPath, [join(checkout, 'scripts/check-prepared-release.mjs')], {
    cwd: f.root, env: { ...process.env, GITHUB_REPOSITORY: repo, GITHUB_OUTPUT: output },
  });
  assert.equal(readFileSync(output, 'utf8'), 'existing=value\nprepared=false\n');
  assert.equal(git(f.root, 'rev-parse', 'HEAD'), head);
  assert.equal(git(f.root, 'diff', '--name-only', 'HEAD'), '');
});

test('a claimed preparation cannot hide code or dependency changes', async (t) => {
  for (const path of ['source.rs', 'desktop/package.json']) {
    const f = fixture(t);
    writeFileSync(join(f.root, path), path.endsWith('.json') ? '{"version":"0.1.0","dependencies":{"extra":"1.0.0"}}\n' : 'changed\n');
    git(f.root, 'add', '.');
    git(f.root, 'commit', '--amend', '--no-edit', '-q');
    assert.equal(await f.check(), false);
    assert.deepEqual(f.requests, []);
  }
});

test('rejects merge commits even when their first-parent diff is version-only', async (t) => {
  const f = fixture(t);
  const other = git(f.root, 'commit-tree', `${f.source}^{tree}`, '-p', f.source, '-m', 'Other');
  const merge = git(f.root, 'commit-tree', 'HEAD^{tree}', '-p', f.source, '-p', other, '-m', 'Release-Preparation: 123');
  git(f.root, 'update-ref', 'HEAD', merge);
  assert.equal(await f.check(), false);
  assert.deepEqual(f.requests, []);
});

test('requires the matching repository, source, branch, workflow and release event', async (t) => {
  const f = fixture(t);
  for (const override of [
    { id: 456 }, { head_sha: 'a'.repeat(40) }, { head_branch: 'feature' },
    { path: '.github/workflows/rust-ci.yml' }, { event: 'pull_request' }, { event: 'push' },
    { repository: { full_name: 'other/repo' } }, { head_repository: { full_name: 'fork/repo' } },
  ]) {
    assert.equal(await f.check({ api: async () => [{ ...f.run, ...override }] }), false);
  }
});

test('never substitutes failed, cancelled or skipped preparation for checks', async (t) => {
  const f = fixture(t);
  for (const conclusion of ['failure', 'cancelled', 'skipped', 'neutral', null]) {
    f.run.conclusion = conclusion;
    assert.equal(await f.check(), false);
  }
  f.run.conclusion = 'success';
  for (const conclusion of ['skipped', 'failure', 'cancelled', null]) {
    f.job.conclusion = conclusion;
    assert.equal(await f.check(), false);
  }
  f.job.conclusion = 'success';
  f.job.status = 'in_progress';
  assert.equal(await f.check(), false);
});

test('requires the successful version job, including when job results are paginated', async (t) => {
  const f = fixture(t);
  for (const jobs of [[], [{ ...f.job, name: 'Check release changes' }], [f.job, f.job]]) {
    assert.equal(await f.check({ api: async (endpoint) => endpoint.includes('/jobs?') ? [{ jobs }] : [f.run] }), false);
  }
  f.run.run_attempt = 2;
  assert.equal(await f.check({ api: async (endpoint) => {
    if (!endpoint.includes('/jobs?')) return [f.run];
    assert.match(endpoint, /\/attempts\/2\/jobs\?/);
    return [{ jobs: [{ name: 'Check release changes' }] }, { jobs: [f.job] }];
  } }), true);
});

test('waits for the originating run to finish after its atomic push', async (t) => {
  const f = fixture(t);
  f.run.status = 'in_progress';
  f.run.conclusion = null;
  assert.equal(await f.check({ wait: async () => {
    f.run.status = 'completed';
    f.run.conclusion = 'success';
  } }), true);
  assert.equal(f.requests.length, 3);
});

test('pending preparation has a bounded wait and then keeps checks', async (t) => {
  const f = fixture(t);
  f.run.status = 'in_progress';
  f.run.conclusion = null;
  assert.equal(await f.check(), false);
  assert.equal(f.waits(), 11);
  assert.equal(f.requests.length, 12);
});

test('API failures and malformed responses keep checks', async (t) => {
  const f = fixture(t);
  assert.equal(await f.check({ api: async () => { throw new Error('API unavailable'); } }), false);
  assert.equal(await f.check({ api: async () => [] }), false);
  assert.equal(await f.check({ api: async (endpoint) => endpoint.includes('/jobs?') ? [{}] : [f.run] }), false);
});
