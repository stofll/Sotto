import { execFileSync } from 'node:child_process';
import { appendFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// The pull request jobs that Prepare Release would otherwise repeat. A renamed
// job only stops the reuse; preparation then runs the full source checks.
const RUST_BUILDS = ['ubuntu-24.04', 'windows-latest', 'macos-latest'].map((os) => `Build + test (${os})`);
export const REQUIRED_JOBS = {
  'rust-ci.yml': [
    'Changed paths', 'Dependency security audit', 'Cargo fmt (Linux)', 'TypeScript typecheck + build',
    'Static gate — no sidecar_invoke / Python runtime', ...RUST_BUILDS,
  ],
  'ui-tests.yml': [
    ...['production', 'dev'].flatMap((mode) => ['chromium', 'webkit'].map((browser) => `browser-tests (${mode}, ${browser})`)),
    'browser-ui',
  ],
};

// Reuse a pull request's checks only for the exact tree it merged into main.
// The checks ran on the PR head, so the merged tree must equal that head's tree.
export async function checkMergedPullRequest({ repo, sha, api, log = console.log }) {
  try {
    const pulls = (await api(`repos/${repo}/commits/${sha}/pulls`)).flat().filter((pull) =>
      pull.merged_at && pull.merge_commit_sha === sha && pull.base?.ref === 'main' && pull.base?.repo?.full_name === repo);
    if (pulls.length !== 1) return keep(log, 'No pull request was merged as this commit');
    const [pull] = pulls;
    const head = pull.head.sha;
    const tree = async (ref) => (await api(`repos/${repo}/commits/${ref}`))[0].commit.tree.sha;
    if (await tree(sha) !== await tree(head)) return keep(log, `#${pull.number} merged a tree its checks did not test`);

    for (const [workflow, names] of Object.entries(REQUIRED_JOBS)) {
      const runs = (await api(`repos/${repo}/actions/workflows/${workflow}/runs?head_sha=${head}&event=pull_request&per_page=100`))
        .flatMap((page) => page.workflow_runs)
        .filter((run) => run.path === `.github/workflows/${workflow}` && run.head_sha === head && run.repository?.full_name === repo);
      // A later run for the same head, such as a reopened PR, supersedes earlier ones.
      const run = runs.sort((a, b) => b.run_number - a.run_number)[0];
      if (run?.status !== 'completed' || run.conclusion !== 'success') return keep(log, `#${pull.number} has no successful ${workflow} run`);
      const jobs = (await api(`repos/${repo}/actions/runs/${run.id}/attempts/${run.run_attempt}/jobs?per_page=100`)).flatMap((page) => page.jobs);
      for (const name of names) {
        const matches = jobs.filter((job) => job.name === name);
        if (matches.length !== 1 || matches[0].conclusion !== 'success') return keep(log, `#${pull.number} did not pass ${name}`);
        // A PR without application changes reports these names from a stand-in job.
        if (RUST_BUILDS.includes(name) && !matches[0].steps?.some((step) => step.name === 'Test' && step.conclusion === 'success')) {
          return keep(log, `#${pull.number} did not run the tests in ${name}`);
        }
      }
    }
    log(`Reusing the checks of #${pull.number}, which merged this exact tree.`);
    return true;
  } catch (error) {
    return keep(log, `Cannot verify pull request checks: ${error.message}`);
  }
}

function keep(log, reason) {
  log(`${reason}; running the full source checks.`);
  return false;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const reused = await checkMergedPullRequest({
    repo: process.env.GITHUB_REPOSITORY, sha: process.env.RELEASE_SOURCE,
    api: (endpoint) => JSON.parse(execFileSync('gh', ['api', '--paginate', '--slurp', endpoint], { encoding: 'utf8', timeout: 15000 })),
  });
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `reused=${reused}\n`);
}
