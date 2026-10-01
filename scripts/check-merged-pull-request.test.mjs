import assert from 'node:assert/strict';
import test from 'node:test';
import { checkMergedPullRequest, REQUIRED_JOBS } from './check-merged-pull-request.mjs';

const repo = 'owner/repo';
const merge = 'm'.repeat(40);
const head = 'h'.repeat(40);

// A fake GitHub API holding one merged pull request with green checks.
function github() {
  const state = {
    pulls: [{
      number: 7, merged_at: '2026-10-01T10:00:00Z', merge_commit_sha: merge,
      base: { ref: 'main', repo: { full_name: repo } }, head: { sha: head },
    }],
    trees: { [merge]: 'tree', [head]: 'tree' },
    runs: {}, jobs: {},
  };
  Object.entries(REQUIRED_JOBS).forEach(([workflow, names], index) => {
    const id = 100 + index;
    state.runs[workflow] = [{
      id, run_number: 1, run_attempt: 1, path: `.github/workflows/${workflow}`, head_sha: head,
      repository: { full_name: repo }, status: 'completed', conclusion: 'success',
    }];
    state.jobs[id] = names.map((name) => ({
      name, conclusion: 'success', steps: name.startsWith('Build + test') ? [{ name: 'Test', conclusion: 'success' }] : [],
    }));
  });
  const requests = [];
  const api = async (endpoint) => {
    requests.push(endpoint);
    let match;
    if ((match = endpoint.match(/^repos\/owner\/repo\/commits\/(\w+)\/pulls$/))) return [state.pulls];
    if ((match = endpoint.match(/^repos\/owner\/repo\/commits\/(\w+)$/))) return [{ commit: { tree: { sha: state.trees[match[1]] } } }];
    if ((match = endpoint.match(/^repos\/owner\/repo\/actions\/workflows\/([\w.-]+)\/runs\?head_sha=(\w+)&event=pull_request/))) {
      return [{ workflow_runs: state.runs[match[1]].filter((run) => run.head_sha === match[2]) }];
    }
    if ((match = endpoint.match(/^repos\/owner\/repo\/actions\/runs\/(\d+)\/attempts\/(\d+)\/jobs/))) return [{ jobs: state.jobs[match[1]] }];
    throw new Error(`Unexpected endpoint ${endpoint}`);
  };
  const check = () => checkMergedPullRequest({ repo, sha: merge, api, log: () => {} });
  return { state, requests, check };
}

test('reuses green checks of the pull request that merged this exact tree', async () => {
  const { check, requests } = github();
  assert.equal(await check(), true);
  assert.ok(requests.some((endpoint) => endpoint.includes('/attempts/1/jobs')));
});

test('keeps source checks for commits that no single pull request merged', async () => {
  for (const pulls of [
    [],
    [{ merged_at: null }],
    [{ merged_at: 'x', merge_commit_sha: head, base: { ref: 'main', repo: { full_name: repo } }, head: { sha: head } }],
    [{ merged_at: 'x', merge_commit_sha: merge, base: { ref: 'feature', repo: { full_name: repo } }, head: { sha: head } }],
    [{ merged_at: 'x', merge_commit_sha: merge, base: { ref: 'main', repo: { full_name: 'other/repo' } }, head: { sha: head } }],
  ]) {
    const f = github();
    f.state.pulls = pulls;
    assert.equal(await f.check(), false);
  }
  const twice = github();
  twice.state.pulls.push({ ...twice.state.pulls[0], number: 8 });
  assert.equal(await twice.check(), false);
});

test('refuses a merged tree that differs from the tested head', async () => {
  const f = github();
  f.state.trees[merge] = 'other tree';
  assert.equal(await f.check(), false);
  assert.ok(!f.requests.some((endpoint) => endpoint.includes('/actions/')));
});

test('requires the latest run of each workflow to succeed', async () => {
  for (const workflow of Object.keys(REQUIRED_JOBS)) {
    for (const change of [{ status: 'in_progress', conclusion: null }, { conclusion: 'failure' }, { conclusion: 'cancelled' },
      { path: '.github/workflows/other.yml' }, { repository: { full_name: 'fork/repo' } }]) {
      const f = github();
      Object.assign(f.state.runs[workflow][0], change);
      assert.equal(await f.check(), false, `${workflow} ${JSON.stringify(change)}`);
    }
    const missing = github();
    missing.state.runs[workflow] = [];
    assert.equal(await missing.check(), false);
    const superseded = github();
    superseded.state.runs[workflow].push({ ...superseded.state.runs[workflow][0], id: 999, run_number: 2, conclusion: 'failure' });
    assert.equal(await superseded.check(), false);
  }
});

test('requires every repeated job, including the real Rust tests', async () => {
  for (const [workflow, names] of Object.entries(REQUIRED_JOBS)) {
    for (const name of names) {
      const f = github();
      const jobs = f.state.jobs[f.state.runs[workflow][0].id];
      jobs.find((job) => job.name === name).conclusion = 'skipped';
      assert.equal(await f.check(), false, name);
    }
  }
  const standIn = github();
  const rust = standIn.state.jobs[standIn.state.runs['rust-ci.yml'][0].id];
  rust.find((job) => job.name === 'Build + test (windows-latest)').steps = [];
  assert.equal(await standIn.check(), false);
});

test('reads jobs from the run attempt that produced the result', async () => {
  const f = github();
  const run = f.state.runs['ui-tests.yml'][0];
  run.run_attempt = 2;
  assert.equal(await f.check(), true);
  assert.ok(f.requests.includes(`repos/${repo}/actions/runs/${run.id}/attempts/2/jobs?per_page=100`));
});

test('API failures keep the source checks', async () => {
  const result = await checkMergedPullRequest({ repo, sha: merge, api: async () => { throw new Error('unavailable'); }, log: () => {} });
  assert.equal(result, false);
});
