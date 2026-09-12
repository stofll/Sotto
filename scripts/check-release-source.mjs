import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const releaseWorkflows = ['rust-ci.yml', 'ui-tests.yml'];

export function latestRunPassed(runs, sha) {
  const latest = runs.filter((run) => run.head_sha === sha && ['pull_request', 'workflow_dispatch', 'push'].includes(run.event))
    .sort((a, b) => b.id - a.id)[0];
  return latest?.status === 'completed' && latest.conclusion === 'success';
}

export function checkReleaseSource({ repo, source, api, git }) {
  const runsFor = (sha) => releaseWorkflows.map((workflow) => {
    const pages = api(`repos/${repo}/actions/workflows/${workflow}/runs?head_sha=${sha}&per_page=100`);
    return pages.flatMap((page) => page.workflow_runs);
  });
  const sourceRuns = runsFor(source);
  if (sourceRuns.every((runs) => latestRunPassed(runs, source))) return source;
  if (sourceRuns.some((runs) => runs.length > 0)) {
    throw new Error('Source CI is incomplete or unsuccessful. Finish successful Rust CI and UI tests on main before releasing.');
  }

  // PR CI is attached to the PR head, not necessarily to the squash/merge SHA.
  // Reuse it only when the final main tree is byte-for-byte identical.
  const pulls = api(`repos/${repo}/commits/${source}/pulls?per_page=100`).flat();
  for (const pr of pulls) {
    if (!pr.merged_at || pr.base.ref !== 'main' || pr.base.repo.full_name !== repo || pr.merge_commit_sha !== source) continue;
    const head = pr.head.sha;
    if (!/^[0-9a-f]{40}$/.test(head)) throw new Error('Invalid PR head SHA');
    git('fetch', '--no-tags', 'origin', head);
    if (git('rev-parse', `${head}^{tree}`) === git('rev-parse', `${source}^{tree}`)
      && runsFor(head).every((runs) => latestRunPassed(runs, head))) return head;
  }
  throw new Error('No successful Rust CI and UI tests for this source tree. Finish PR CI or run both workflows manually on main, then retry.');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const repo = process.env.GITHUB_REPOSITORY;
    const source = process.env.RELEASE_SOURCE;
    if (!repo || !/^[0-9a-f]{40}$/.test(source ?? '')) throw new Error('Repository and source SHA are required');
    const checked = checkReleaseSource({
      repo, source,
      api: (endpoint) => JSON.parse(execFileSync('gh', ['api', '--paginate', '--slurp', endpoint], { encoding: 'utf8' })),
      git: (...args) => execFileSync('git', args, { encoding: 'utf8' }).trim(),
    });
    console.log(`Source ${source} matches successful CI on ${checked}`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
