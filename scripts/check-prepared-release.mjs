import { execFileSync } from 'node:child_process';
import { appendFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout } from 'node:timers/promises';
import { verifyReleaseCommit } from './release-version.mjs';

// Reuse source checks only across the exact version-only commit made by a
// successful preparation. A bot name or commit subject alone is not proof.
export async function checkPreparedRelease({ root, repo, before, api, wait = setTimeout, log = console.log }) {
  try {
    const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
    const markers = [...git('show', '-s', '--format=%B', 'HEAD').matchAll(/^Release-Preparation: ([1-9]\d*)$/gm)];
    if (markers.length !== 1) return false;
    const source = git('show', '-s', '--format=%P', 'HEAD');
    if (!/^[0-9a-f]{40}$/.test(source) || (before !== undefined && before !== source)) return false;
    const version = JSON.parse(git('show', 'HEAD:desktop/package.json')).version;
    verifyReleaseCommit(root, source, version);

    const endpoint = `repos/${repo}/actions/runs/${markers[0][1]}`;
    // The atomic push starts consumers before Prepare Release finishes its
    // cleanup. Wait briefly for completion, then fall back to ordinary checks.
    for (let attempt = 0; attempt < 12; attempt++) {
      const [run] = await api(endpoint);
      if (String(run.id) !== markers[0][1] || run.path !== '.github/workflows/prepare-release.yml'
        || run.repository?.full_name !== repo || run.head_repository?.full_name !== repo
        || run.head_branch !== 'main' || run.head_sha !== source
        || !['workflow_dispatch', 'schedule'].includes(run.event)) return false;
      if (run.status === 'completed') {
        if (run.conclusion !== 'success') return false;
        const pages = await api(`${endpoint}/attempts/${run.run_attempt}/jobs?per_page=100`);
        const preparation = pages.flatMap((page) => page.jobs).filter((job) => job.name === 'Commit and tag release version');
        // An unchanged scheduled run can succeed with all source jobs skipped.
        return preparation.length === 1 && preparation[0].status === 'completed' && preparation[0].conclusion === 'success';
      }
      if (attempt < 11) await wait(5000);
    }
    log('Preparation is still pending; keeping ordinary checks.');
  } catch (error) {
    log(`Cannot verify release preparation; keeping ordinary checks: ${error.message}`);
  }
  return false;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const prepared = await checkPreparedRelease({
    root: process.cwd(), repo: process.env.GITHUB_REPOSITORY, before: process.env.RELEASE_BASE,
    api: (endpoint) => JSON.parse(execFileSync('gh', ['api', '--paginate', '--slurp', endpoint], { encoding: 'utf8', timeout: 15000 })),
  });
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `prepared=${prepared}\n`);
  console.log(prepared ? 'Verified Prepare Release checks; skipping duplicate work.' : 'No verified preparation; keeping ordinary checks.');
}
