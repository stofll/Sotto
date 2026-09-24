import assert from 'node:assert/strict';
import test from 'node:test';
import { checkReleaseSource, latestRunPassed } from './check-release-source.mjs';

const source = 'a'.repeat(40);
const head = 'b'.repeat(40);
const run = (sha, overrides = {}) => ({ id: 1, head_sha: sha, event: 'pull_request', status: 'completed', conclusion: 'success', ...overrides });

test('requires the latest CI attempt to succeed, never an older green result', () => {
  assert.equal(latestRunPassed([run(source)], source), true);
  for (const overrides of [{ conclusion: 'failure' }, { conclusion: 'cancelled' }, { conclusion: 'skipped' }, { status: 'in_progress', conclusion: null }]) {
    assert.equal(latestRunPassed([run(source), run(source, { id: 2, ...overrides })], source), false);
  }
  assert.equal(latestRunPassed([run(head)], source), false);
  assert.equal(latestRunPassed([run(source, { event: 'pull_request_target' })], source), false);
  assert.equal(latestRunPassed([run(source, { event: 'push' })], source), false);
});

function verify({
  direct = false, sameTree = true, merged = true, uiPass = true,
  sourcePush = null, sourceUiDispatch = false, sourceFailed = false, sourcePending = false,
  headFailed = false, headPending = false, apiFails = false,
} = {}) {
  return checkReleaseSource({
    repo: 'owner/repo', source,
    api: (endpoint) => {
      if (apiFails) throw new Error('API unavailable');
      if (endpoint.includes('/pulls?')) return [[{
        merged_at: merged ? '2026-09-12' : null,
        base: { ref: 'main', repo: { full_name: 'owner/repo' } },
        merge_commit_sha: source, head: { sha: head },
      }]];
      const sha = endpoint.includes(`head_sha=${source}`) ? source : head;
      const uiWorkflow = endpoint.includes('ui-tests.yml');
      let runs = (direct || sha === head) ? [run(sha)] : [];
      if (sourcePush && sha === source && !uiWorkflow) {
        const pending = sourcePush === 'in_progress';
        runs = [run(sha, { event: 'push', status: pending ? 'in_progress' : 'completed', conclusion: pending ? null : sourcePush })];
      }
      if (sourceUiDispatch && sha === source && uiWorkflow) runs = [run(sha, { event: 'workflow_dispatch' })];
      if (sourceFailed && sha === source) runs = [run(sha, { conclusion: 'failure' })];
      if (sourcePending && sha === source) runs = [run(sha, { status: 'in_progress', conclusion: null })];
      if (headFailed && sha === head && uiWorkflow) runs = [run(sha, { conclusion: 'failure' })];
      if (headPending && sha === head && uiWorkflow) runs = [run(sha, { status: 'in_progress', conclusion: null })];
      if (!uiPass && uiWorkflow) runs = [];
      return [{ workflow_runs: runs }];
    },
    git: (command, ...args) => command === 'fetch' ? '' : (sameTree || args[0].startsWith(source) ? 'tree-1' : 'tree-2'),
  });
}

test('accepts successful CI directly on the source commit', () => assert.equal(verify({ direct: true }), source));
test('reuses merged PR CI only for an identical tree', () => {
  assert.equal(verify(), head);
  assert.throws(() => verify({ sameTree: false }));
  assert.throws(() => verify({ merged: false }));
});
test('ignores the build-only Rust CI run of a push to main', () => {
  for (const sourcePush of ['success', 'failure', 'in_progress']) {
    assert.equal(verify({ sourcePush }), head);
    assert.throws(() => verify({ sourcePush, sameTree: false }), /No successful/);
    assert.throws(() => verify({ sourcePush, headFailed: true }), /No successful/);
    assert.throws(() => verify({ sourcePush, headPending: true }), /No successful/);
  }
  assert.throws(() => verify({ sourcePush: 'success', sourceUiDispatch: true, merged: false }), /No successful/);
});
test('refuses missing UI coverage and fails closed on API errors', () => {
  assert.throws(() => verify({ uiPass: false }));
  assert.throws(() => verify({ apiFails: true }), /API unavailable/);
});
test('does not hide a failed main check behind earlier successful PR CI', () => {
  assert.throws(() => verify({ sourceFailed: true }), /unsuccessful/);
  assert.throws(() => verify({ sourcePending: true }), /unsuccessful/);
});
